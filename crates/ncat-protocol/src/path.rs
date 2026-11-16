//! Same-host IPC path encoding, NOT a cross-platform persisted path format.
//! Wire v1 preserves Unix bytes, including non-UTF-8 names. A Windows port must
//! define and validate a lossless native encoding and update the wire contract.
use serde::{Deserialize, Serialize};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RawPath(pub Vec<u8>);

impl RawPath {
    pub fn new(path: &Path) -> Self {
        Self(path.as_os_str().as_bytes().to_vec())
    }
    pub fn path(&self) -> PathBuf {
        std::ffi::OsString::from_vec(self.0.clone()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_path_round_trip_is_lossless() {
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/book/\xff.png".to_vec()));
        assert_eq!(RawPath::new(&path).path(), path);
    }
}
