//! Find one naturally ordered sibling without allocating a directory catalog.
use ncat_core::natural_cmp;
use ncat_protocol::{ErrorCode, MAX_ENTRIES, MAX_PATH_BYTES, RawPath, image_filename_hint};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn archive_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip") || ext.eq_ignore_ascii_case("cbz"))
}

pub(crate) fn find(
    source: &Path,
    zip: bool,
    forward: bool,
    current: impl Fn() -> bool,
) -> Result<Option<PathBuf>, ErrorCode> {
    // Normalize `.` and relative paths so opening an image inside a folder
    // anchors browsing to that folder, not to the individual image.
    let source = fs::canonicalize(source).map_err(|_| ErrorCode::Io)?;
    let anchor = if zip || source.is_dir() {
        source.as_path()
    } else {
        source.parent().ok_or(ErrorCode::Io)?
    };
    let (Some(parent), Some(name)) = (anchor.parent(), anchor.file_name()) else {
        return Ok(None);
    };
    let compare = |a: &std::ffi::OsStr, b: &std::ffi::OsStr| {
        natural_cmp(a.as_encoded_bytes(), b.as_encoded_bytes())
    };
    let direction = if forward {
        Ordering::Greater
    } else {
        Ordering::Less
    };
    let mut best: Option<PathBuf> = None;
    let mut entries = 0;
    let mut count = || {
        if !current() {
            return Err(ErrorCode::Cancelled);
        }
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(ErrorCode::Limit);
        }
        Ok(())
    };
    for entry in fs::read_dir(parent).map_err(|_| ErrorCode::Io)? {
        count()?;
        let entry = entry.map_err(|_| ErrorCode::Io)?;
        let candidate = entry.file_name();
        if compare(&candidate, name) != direction
            || best
                .as_ref()
                .and_then(|p| p.file_name())
                .is_some_and(|previous| compare(previous, &candidate) != direction)
        {
            continue;
        }
        let kind = entry.file_type().map_err(|_| ErrorCode::Io)?;
        if !kind.is_dir() && !(kind.is_file() && archive_name(Path::new(&candidate))) {
            continue;
        }
        let path = entry.path();
        if RawPath::new(&path).0.len() > MAX_PATH_BYTES {
            continue;
        }
        if kind.is_dir() {
            let Ok(children) = fs::read_dir(&path) else {
                continue;
            };
            let mut has_image = false;
            for child in children {
                count()?;
                if let Ok(child) = child
                    && image_filename_hint(child.file_name().as_encoded_bytes())
                    && child.file_type().is_ok_and(|kind| kind.is_file())
                {
                    has_image = true;
                    break;
                }
            }
            if !has_image {
                continue;
            }
        }
        best = Some(path);
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_siblings_are_naturally_ordered_and_images_anchor_to_their_folder() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Book 2");
        let archive = root.path().join("Book 10.CBZ");
        let later = root.path().join("Book 14");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(&later).unwrap();
        fs::write(later.join("page.png"), []).unwrap();
        fs::create_dir(root.path().join("Book 12")).unwrap();
        fs::write(&archive, []).unwrap();
        fs::write(root.path().join("Book 3.png"), []).unwrap();
        let image = folder.join("page.png");
        fs::write(&image, []).unwrap();
        let file_name = |path: &Path, zip, forward| {
            find(path, zip, forward, || true)
                .unwrap()
                .and_then(|p| p.file_name().map(|name| name.to_owned()))
        };
        assert_eq!(
            file_name(&image, false, true),
            archive.file_name().map(|n| n.to_owned())
        );
        assert_eq!(
            file_name(&archive, true, false),
            folder.file_name().map(|n| n.to_owned())
        );
        assert_eq!(
            file_name(&archive, true, true),
            later.file_name().map(|n| n.to_owned())
        );
        assert_eq!(file_name(&later, false, true), None);
        assert_eq!(
            find(&folder, false, true, || false),
            Err(ErrorCode::Cancelled)
        );
    }
}
