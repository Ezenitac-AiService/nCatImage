use ncat_protocol::ErrorCode;
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

/// Raw native name bytes for stable sorting and sanitized display labels.
pub fn name_bytes(name: &std::ffi::OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified: (i64, i64),
    pub changed: (i64, i64),
}

impl Revision {
    pub fn differences(&self, now: &Self) -> String {
        format!(
            "stage=revision_comparison identity_changed={} size_changed={} mtime_changed={} ctime_changed={}",
            (self.device, self.inode) != (now.device, now.inode),
            self.size != now.size,
            self.modified != now.modified,
            self.changed != now.changed
        )
    }

    pub fn read(path: &Path) -> Result<Self, ErrorCode> {
        Self::metadata(&fs::metadata(path).map_err(|_| ErrorCode::Io)?)
    }

    pub fn of(file: &File) -> Result<Self, ErrorCode> {
        Self::metadata(&file.metadata().map_err(|_| ErrorCode::Io)?)
    }

    fn metadata(m: &fs::Metadata) -> Result<Self, ErrorCode> {
        if !m.is_file() {
            return Err(ErrorCode::Unsupported);
        }
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
            size: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        })
    }
}

pub fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}

pub fn create_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Only opens an existing parent-owned output, never follows its final symlink.
pub fn open_pixel_output(path: &Path) -> io::Result<File> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::TRUNC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )?;
    let file = File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn private_files_and_output_keep_security_contract() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        create_private_directory(&directory).unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = directory.join("pixels");
        create_private_file(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(create_private_file(&path).is_err());
        let link = directory.join("link");
        symlink(&path, &link).unwrap();
        assert!(open_pixel_output(&link).is_err());
        assert!(open_pixel_output(&directory.join("missing")).is_err());
        assert!(open_pixel_output(&path).is_ok());
    }

    #[test]
    fn replacement_changes_identity_even_with_equal_length() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("page");
        fs::write(&path, b"old").unwrap();
        let old = Revision::read(&path).unwrap();
        let new = root.path().join("replacement");
        fs::write(&new, b"new").unwrap();
        fs::rename(new, &path).unwrap();
        let now = Revision::read(&path).unwrap();
        assert_ne!(old, now);
        assert!(old.differences(&now).contains("identity_changed=true"));
        assert!(Revision::read(root.path()).is_err());
    }
}
