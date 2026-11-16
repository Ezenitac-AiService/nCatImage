//! Delegate remote mounts/authentication to the user's desktop session. Never
//! log/debug a GFile, URI, D-Bus reply or arbitrary GLib error from this module.
use gtk::{gio, glib, prelude::*};
#[cfg(test)]
use ncat_backend::NetworkProtocol;
use ncat_backend::{NetworkBridge, NetworkFailure};
#[cfg(test)]
use ncat_gtk::network::{Location, classify};
use std::path::PathBuf;

fn failure(error: &glib::Error) -> NetworkFailure {
    use gio::{DBusError, IOErrorEnum};
    if error.matches(IOErrorEnum::Cancelled) {
        NetworkFailure::Cancelled
    } else if error.matches(IOErrorEnum::TimedOut)
        || error.matches(DBusError::Timeout)
        || error.matches(DBusError::NoReply)
    {
        NetworkFailure::Timeout
    } else if error.matches(IOErrorEnum::PermissionDenied) || error.matches(DBusError::AccessDenied)
    {
        NetworkFailure::Permission
    } else if error.matches(IOErrorEnum::NotFound) {
        NetworkFailure::NotFound
    } else if error.matches(DBusError::ServiceUnknown)
        || error.matches(DBusError::NameHasNoOwner)
        || error.matches(DBusError::SpawnExecFailed)
        || error.matches(IOErrorEnum::NotSupported)
    {
        NetworkFailure::Unavailable
    } else if error.matches(IOErrorEnum::HostNotFound)
        || error.matches(IOErrorEnum::ConnectionRefused)
        || error.matches(IOErrorEnum::NetworkUnreachable)
        || error.matches(IOErrorEnum::HostUnreachable)
    {
        NetworkFailure::Connection
    } else {
        NetworkFailure::Other
    }
}

fn mounted_path(reply: &glib::Variant) -> Result<PathBuf, NetworkFailure> {
    let (path,) = reply
        .get::<(String,)>()
        .ok_or(NetworkFailure::InvalidReply)?;
    let path = PathBuf::from(path);
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(NetworkFailure::InvalidReply);
    }
    Ok(path)
}

pub async fn resolve(
    file: gio::File,
    parent: gtk::ApplicationWindow,
) -> Result<(PathBuf, NetworkBridge), NetworkFailure> {
    if let Some(path) = file.path() {
        return Ok((path, NetworkBridge::Gvfs));
    }
    let kio_result = match gio::bus_get_future(gio::BusType::Session).await {
        Ok(bus) => {
            let parameters = (file.uri().as_str(),).to_variant();
            bus.call_future(
                Some("org.kde.KIOFuse"),
                "/org/kde/KIOFuse",
                "org.kde.KIOFuse.VFS",
                "mountUrl",
                Some(&parameters),
                Some(glib::VariantTy::new("(s)").unwrap()),
                gio::DBusCallFlags::NONE,
                60_000,
            )
            .await
            .map_err(|error| failure(&error))
            .and_then(|reply| mounted_path(&reply))
        }
        Err(_) => Err(NetworkFailure::Unavailable),
    };
    match kio_result {
        Ok(path) => return Ok((path, NetworkBridge::KioFuse)),
        Err(NetworkFailure::Unavailable) => {}
        Err(reason) => return Err(reason),
    }
    // GNOME/GVfs fallback: remote GIO backends AND gvfs-fuse are required.
    // Do not download a whole book into the GUI as a fallback for missing FUSE.
    let scheme = file.uri_scheme().ok_or(NetworkFailure::InvalidUri)?;
    if !gio::Vfs::default()
        .supported_uri_schemes()
        .iter()
        .any(|s| s == &scheme)
    {
        return Err(NetworkFailure::Unavailable);
    }
    let operation = gtk::MountOperation::new(Some(&parent));
    match file
        .mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&operation))
        .await
    {
        Ok(()) => {}
        Err(error) if error.matches(gio::IOErrorEnum::AlreadyMounted) => {}
        Err(error) => return Err(failure(&error)),
    }
    let path = file.path().ok_or(NetworkFailure::NoLocalPath)?;
    Ok((path, NetworkBridge::Gvfs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_only_requested_network_protocols() {
        for (uri, expected) in [
            ("smb://server/share/folder", NetworkProtocol::Smb),
            ("ftp://server:2121/book.cbz", NetworkProtocol::Ftp),
            (
                "sftp://user@server:2222/book%20name.cbz",
                NetworkProtocol::Sftp,
            ),
        ] {
            assert!(
                matches!(classify(gio::File::for_uri(uri)), Ok(Location::Remote { protocol, .. }) if protocol == expected)
            );
        }
        assert!(matches!(
            classify(gio::File::for_uri("https://server/book.cbz")),
            Err(NetworkFailure::UnsupportedScheme)
        ));
    }

    #[test]
    fn validates_addresses_and_does_not_accept_embedded_passwords() {
        assert!(matches!(
            classify(gio::File::for_uri("smb:///share/book.cbz")),
            Err(NetworkFailure::InvalidUri)
        ));
        assert!(matches!(
            classify(gio::File::for_uri("sftp://user:secret@server/book.cbz")),
            Err(NetworkFailure::EmbeddedPassword)
        ));
    }

    #[test]
    fn preserves_local_non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/mounted/\xff.png".to_vec()));
        assert!(
            matches!(classify(gio::File::for_path(&path)), Ok(Location::Local(found)) if found == path)
        );
    }

    #[test]
    fn rejects_invalid_bridge_replies() {
        assert_eq!(
            mounted_path(&("relative/book.cbz",).to_variant()),
            Err(NetworkFailure::InvalidReply)
        );
        assert_eq!(
            mounted_path(&42u32.to_variant()),
            Err(NetworkFailure::InvalidReply)
        );
        assert_eq!(
            mounted_path(&("/run/user/1000/kio-fuse/mounted/book.cbz",).to_variant()).unwrap(),
            PathBuf::from("/run/user/1000/kio-fuse/mounted/book.cbz")
        );
    }
}
