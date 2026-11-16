//! GIO location validation and the injected OS mount service.
//! URI/error contents must never be logged.
use gtk::{gio, glib, prelude::*};
use ncat_backend::{NetworkBridge, NetworkFailure, NetworkProtocol};
use std::{future::Future, path::PathBuf, pin::Pin};

pub type Resolution =
    Pin<Box<dyn Future<Output = Result<(PathBuf, NetworkBridge), NetworkFailure>>>>;
/// The shell supplies authentication/mount integration; dropping the future cancels app waiting.
pub type Resolver = fn(gio::File, gtk::ApplicationWindow) -> Resolution;

pub enum Location {
    Local(PathBuf),
    Remote {
        file: gio::File,
        protocol: NetworkProtocol,
    },
}

pub fn classify(file: gio::File) -> Result<Location, NetworkFailure> {
    if file.is_native()
        && let Some(path) = file.path()
    {
        return Ok(Location::Local(path));
    }
    let text = file.uri();
    if text.len() > 16 * 1024 {
        return Err(NetworkFailure::InvalidUri);
    }
    let uri = glib::Uri::parse(&text, glib::UriFlags::HAS_PASSWORD)
        .map_err(|_| NetworkFailure::InvalidUri)?;
    let protocol = match uri.scheme().to_ascii_lowercase().as_str() {
        "smb" => NetworkProtocol::Smb,
        "ftp" => NetworkProtocol::Ftp,
        "sftp" => NetworkProtocol::Sftp,
        _ => return Err(NetworkFailure::UnsupportedScheme),
    };
    if uri.host().is_none_or(|host| host.is_empty())
        || (!uri.path().is_empty() && !uri.path().starts_with('/'))
    {
        return Err(NetworkFailure::InvalidUri);
    }
    if uri.password().is_some() {
        return Err(NetworkFailure::EmbeddedPassword);
    }
    Ok(Location::Remote { file, protocol })
}

pub fn message(reason: NetworkFailure) -> &'static str {
    match reason {
        NetworkFailure::UnsupportedScheme => "SMB·FTP·SFTP 또는 로컬 경로만 지원합니다",
        NetworkFailure::InvalidUri => "네트워크 주소 형식이 올바르지 않습니다",
        NetworkFailure::EmbeddedPassword => {
            "비밀번호 포함 URL 대신 파일 관리자에서 로그인한 뒤 열어 주세요"
        }
        NetworkFailure::Unavailable => "OS 네트워크 연결 도구가 없습니다: OS에서 먼저 마운트하세요",
        NetworkFailure::NoLocalPath => {
            "네트워크 폴더를 로컬 경로로 연결할 수 없습니다: OS 마운트를 확인하세요"
        }
        NetworkFailure::Permission => {
            "네트워크 접근 권한을 확인하고 파일 관리자에서 로그인해 주세요"
        }
        NetworkFailure::NotFound => "네트워크 파일 또는 폴더를 찾을 수 없습니다",
        NetworkFailure::Connection => "네트워크 서버에 연결할 수 없습니다",
        NetworkFailure::Timeout => "네트워크 폴더 연결 시간이 초과되었습니다",
        NetworkFailure::Cancelled => "네트워크 폴더 연결을 취소했습니다",
        NetworkFailure::InvalidReply => "네트워크 연결 도구의 응답이 올바르지 않습니다",
        NetworkFailure::Other => {
            "네트워크 연결에 실패했습니다: 파일 관리자에서 접속 상태를 확인하세요"
        }
    }
}
