#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
mod path;
pub use path::RawPath;

pub const VERSION: u16 = 4;
pub const MAX_ANIMATION_FRAMES: u32 = 10_000;
pub const MIN_FRAME_DELAY_MS: u32 = 10;
pub const MAX_FRAME_DELAY_MS: u32 = 60_000;
pub const MAX_MESSAGE: usize = 256 * 1024;
pub const MAX_ENTRIES: usize = 100_000;
pub const INDEX_BYTES: u64 = 24 * 1024 * 1024;
pub const MAX_INPUT: u64 = 512 * 1024 * 1024;
pub const MAX_OUTPUT: u64 = 8 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 150_000_000;
// Wire/security policy: shared by parent and worker, not user-overridable.
pub const MAX_PATH_BYTES: usize = 16 * 1024;
pub const MAX_LABEL_CHARS: usize = 512;
pub const MAX_LABEL_BYTES: usize = MAX_LABEL_CHARS * 4;
pub const PRESENTATION_COPIES: u64 = 3;
pub const MAX_VIEW_DIMENSION: u32 = 8192;
pub const PAGE_BATCH_BYTES: u64 = 30 * 1024;
pub const PAGE_BATCH_ENTRIES: usize = 128;
pub const WORKER_ADDRESS_SPACE: u64 = 1024 * 1024 * 1024;
pub const DECODE_OVERHEAD: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PageKey {
    File(RawPath),
    Zip(u64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub key: PageKey,
    pub name: String,
}
impl Page {
    pub fn cost(&self) -> u64 {
        let path = match &self.key {
            PageKey::File(p) => p.0.len(),
            PageKey::Zip(_) => 0,
        };
        (path + self.name.len() + 128) as u64
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub version: u16,
    pub id: u64,
    pub body: T,
}
impl<T> Envelope<T> {
    pub fn new(id: u64, body: T) -> Self {
        Self {
            version: VERSION,
            id,
            body,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Open {
        source: RawPath,
    },
    Catalog,
    Decode {
        key: PageKey,
        output: RawPath,
        max_width: u32,
        max_height: u32,
        workspace: u64,
        quarter_turns: u8,
    },
    AnimationNext {
        output: RawPath,
    },
    AnimationSeek {
        output: RawPath,
        index: u32,
    },
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    Io,
    Unsupported,
    Corrupt,
    Encrypted,
    Limit,
    Changed,
    Protocol,
    Worker,
    Timeout,
    Cancelled,
}
impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io => "파일을 읽을 수 없습니다",
            Self::Unsupported => "아직 지원하지 않는 형식입니다",
            Self::Corrupt => "손상되었거나 읽을 수 없는 이미지입니다",
            Self::Encrypted => "암호화 압축은 지원하지 않습니다",
            Self::Limit => "크기 또는 메모리 한도를 초과했습니다",
            Self::Changed => "원본이 변경되었습니다. 다시 열어 주세요",
            Self::Protocol => "워커 응답이 올바르지 않습니다",
            Self::Worker => "워커가 종료되었습니다",
            Self::Timeout => "작업 제한 시간을 초과했습니다",
            Self::Cancelled => "작업이 취소되었습니다",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimationInfo {
    pub index: u32,
    pub total: Option<u32>,
    pub cycle: u32,
    pub delay_ms: u32,
    /// Persistent decoder/compositing reservation, retained by the parent.
    pub workspace_bytes: u64,
}

impl AnimationInfo {
    pub fn valid(&self) -> bool {
        self.index < MAX_ANIMATION_FRAMES
            && self
                .total
                .is_none_or(|total| total > self.index && total <= MAX_ANIMATION_FRAMES)
            && (MIN_FRAME_DELAY_MS..=MAX_FRAME_DELAY_MS).contains(&self.delay_ms)
            && (DECODE_OVERHEAD..=WORKER_ADDRESS_SPACE).contains(&self.workspace_bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub stride: u64,
    pub bytes: u64,
    pub format: String,
    pub elapsed_us: u64,
    pub animation: Option<AnimationInfo>,
}
impl FrameInfo {
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_VIEW_DIMENSION
            && self.height <= MAX_VIEW_DIMENSION
            && self.original_width > 0
            && self.original_height > 0
            && u64::from(self.original_width) * u64::from(self.original_height) <= MAX_PIXELS
            && self.stride == u64::from(self.width) * 4
            && self.stride.checked_mul(u64::from(self.height)) == Some(self.bytes)
            && self.bytes <= MAX_OUTPUT
            && self.format.len() <= 32
            && self.animation.as_ref().is_none_or(AnimationInfo::valid)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Opened {
        title: String,
        deferred: bool,
    },
    Pages {
        pages: Vec<Page>,
        selected: usize,
        done: bool,
    },
    Frame(FrameInfo),
    AnimationEnded,
    Error(ErrorCode),
    Bye,
}

pub fn write_message<T: Serialize>(writer: &mut impl Write, value: &Envelope<T>) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message limit"));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn read_message<T: for<'de> Deserialize<'de>>(
    reader: &mut impl Read,
) -> io::Result<Option<Envelope<T>>> {
    let mut length = [0; 4];
    match reader.read(&mut length[..1])? {
        0 => return Ok(None),
        1 => {}
        _ => unreachable!(),
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message limit"));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let message: Envelope<T> = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid message"))?;
    if message.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol version",
        ));
    }
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_timing_and_workspace_are_bounded() {
        let info = AnimationInfo {
            index: 0,
            total: None,
            cycle: 0,
            delay_ms: 10,
            workspace_bytes: DECODE_OVERHEAD,
        };
        assert!(info.valid());
        assert!(
            AnimationInfo {
                total: Some(1),
                ..info.clone()
            }
            .valid()
        );
        assert!(
            !AnimationInfo {
                total: Some(0),
                ..info.clone()
            }
            .valid()
        );
        assert!(
            !AnimationInfo {
                total: Some(MAX_ANIMATION_FRAMES + 1),
                ..info.clone()
            }
            .valid()
        );
        assert!(
            !AnimationInfo {
                delay_ms: 0,
                ..info.clone()
            }
            .valid()
        );
        assert!(
            !AnimationInfo {
                index: MAX_ANIMATION_FRAMES,
                ..info.clone()
            }
            .valid()
        );
        assert!(
            !AnimationInfo {
                workspace_bytes: WORKER_ADDRESS_SPACE + 1,
                ..info
            }
            .valid()
        );
    }
    #[test]
    fn bounded_and_versioned_messages() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Envelope::new(7, Request::Catalog)).unwrap();
        let got = read_message::<Request>(&mut bytes.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(got.id, 7);
        assert!(read_message::<Request>(&mut (u32::MAX.to_le_bytes()).as_slice()).is_err());
        assert!(read_message::<Request>(&mut [1_u8, 0].as_slice()).is_err());
        assert!(
            read_message::<Request>(&mut [].as_slice())
                .unwrap()
                .is_none()
        );
        let mut bytes = Vec::new();
        write_message(
            &mut bytes,
            &Envelope {
                version: 0,
                id: 1,
                body: Request::Catalog,
            },
        )
        .unwrap();
        assert!(read_message::<Request>(&mut bytes.as_slice()).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn non_utf8_path_roundtrip() {
        let raw = RawPath(vec![b'/', 0xff, b'a']);
        let data = serde_json::to_vec(&raw).unwrap();
        let copy: RawPath = serde_json::from_slice(&data).unwrap();
        assert_eq!(RawPath::new(&copy.path()), raw);
    }
    #[test]
    fn invalid_frame_rejected() {
        let frame = FrameInfo {
            width: 1,
            height: 1,
            original_width: 1,
            original_height: 1,
            stride: 4,
            bytes: 4,
            format: "Png".into(),
            elapsed_us: 0,
            animation: None,
        };
        assert!(frame.valid());
        assert!(
            !FrameInfo {
                width: 8193,
                height: 1,
                original_width: 8193,
                original_height: 1,
                stride: 8193 * 4,
                bytes: 8193 * 4,
                ..frame.clone()
            }
            .valid()
        );
        assert!(
            !FrameInfo {
                stride: u64::MAX,
                ..frame.clone()
            }
            .valid()
        );
        assert!(
            !FrameInfo {
                bytes: MAX_OUTPUT + 1,
                ..frame
            }
            .valid()
        );
    }
}
