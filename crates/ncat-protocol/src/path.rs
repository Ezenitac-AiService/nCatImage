//! Same-host IPC path encoding, NOT a cross-platform persisted path format.
//! Unix uses raw bytes; Windows uses little-endian UTF-16 code units, including
//! unpaired surrogates. Parent/worker must run on the same host and wire version.
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct RawPath(pub Vec<u8>);

impl RawPath {
    pub fn new(path: &Path) -> Self {
        #[cfg(unix)]
        {
            Self(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            Self(
                path.as_os_str()
                    .encode_wide()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            )
        }
    }

    pub fn valid(&self) -> bool {
        if self.0.len() > crate::MAX_PATH_BYTES {
            return false;
        }
        #[cfg(unix)]
        {
            !self.0.contains(&0)
        }
        #[cfg(windows)]
        {
            let (units, remainder) = self.0.as_chunks::<2>();
            remainder.is_empty() && !units.contains(&[0, 0])
        }
    }

    pub fn path(&self) -> PathBuf {
        #[cfg(unix)]
        {
            std::ffi::OsString::from_vec(self.0.clone()).into()
        }
        #[cfg(windows)]
        {
            // Wire input is validated by Deserialize; direct tuple construction
            // must also preserve native code-unit alignment.
            assert!(self.0.len().is_multiple_of(2), "unaligned native IPC path");
            let wide: Vec<u16> = self
                .0
                .as_chunks::<2>()
                .0
                .iter()
                .map(|unit| u16::from_le_bytes(*unit))
                .collect();
            std::ffi::OsString::from_wide(&wide).into()
        }
    }
}

impl<'de> Deserialize<'de> for RawPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Self(Vec::<u8>::deserialize(deserializer)?);
        if !raw.valid() {
            return Err(serde::de::Error::custom("invalid native IPC path"));
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn native_path_round_trip_is_lossless() {
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/book/\xff.png".to_vec()));
        assert_eq!(RawPath::new(&path).path(), path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_keep_unicode_and_unpaired_surrogates() {
        for wide in [
            "C:\\한글 폴더\\😀.png".encode_utf16().collect::<Vec<_>>(),
            vec![
                b'C' as u16,
                b':' as u16,
                b'\\' as u16,
                0xd800,
                b'.' as u16,
                b'p' as u16,
            ],
        ] {
            let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));
            let raw = RawPath::new(&path);
            let encoded = serde_json::to_vec(&raw).unwrap();
            let copy: RawPath = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(copy.path(), path);
        }
        assert!(serde_json::from_str::<RawPath>("[1]").is_err());
        assert!(serde_json::from_str::<RawPath>("[0,0]").is_err());
    }

    #[test]
    fn invalid_paths_are_rejected_during_deserialization() {
        let too_long = vec![1_u8; crate::MAX_PATH_BYTES + 2];
        assert!(
            serde_json::from_slice::<RawPath>(&serde_json::to_vec(&too_long).unwrap()).is_err()
        );
        assert!(serde_json::from_str::<RawPath>("[0,0]").is_err());
    }
}
