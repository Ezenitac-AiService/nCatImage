//! Windows address policy. Credentials and non-filesystem protocols stay with
//! the OS; never launch a shell or turn a URL into command-line arguments.
use std::path::PathBuf;

fn decode(text: &str) -> Result<String, &'static str> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|n| n as u8);
            let high = input.next().and_then(hex).ok_or("잘못된 URI 인코딩")?;
            let low = input.next().and_then(hex).ok_or("잘못된 URI 인코딩")?;
            bytes.push(high * 16 + low);
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8(bytes).map_err(|_| "URI는 UTF-8이어야 합니다")?;
    if decoded.chars().any(char::is_control) {
        return Err("주소에 제어 문자를 사용할 수 없습니다");
    }
    Ok(decoded)
}

pub fn parse(text: &str) -> Result<PathBuf, &'static str> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > ncat_protocol::MAX_PATH_BYTES
        || text.chars().any(char::is_control)
    {
        return Err("주소 형식이 올바르지 않습니다");
    }
    let Some((scheme, rest)) = text.split_once("://") else {
        // Literal filesystem paths do not percent-decode.
        return Ok(PathBuf::from(text));
    };
    if !scheme.eq_ignore_ascii_case("smb") && !scheme.eq_ignore_ascii_case("file") {
        return Err("FTP/SFTP 등은 OS에서 드라이브로 연결한 뒤 여세요");
    }
    if rest.contains(['?', '#', '\\']) {
        return Err("URI query/fragment 또는 역슬래시는 지원하지 않습니다");
    }
    let (authority, path) = rest.split_once('/').ok_or("공유/파일 경로가 필요합니다")?;
    let authority = decode(authority)?;
    if authority.contains(['@', ':', '/', '\\', '%']) {
        return Err("서버 인증은 OS에서 먼저 연결하세요");
    }
    let path = decode(path)?.replace('/', "\\");
    if scheme.eq_ignore_ascii_case("file")
        && (authority.is_empty() || authority.eq_ignore_ascii_case("localhost"))
    {
        let bytes = path.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || bytes[2] != b'\\'
        {
            return Err("file URI에는 절대 드라이브 경로가 필요합니다");
        }
        return Ok(path.into());
    }
    if authority.is_empty() || path.is_empty() || path.starts_with('\\') {
        return Err("서버 및 공유 경로가 필요합니다");
    }
    Ok(format!("\\\\{authority}\\{path}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_are_literal() {
        assert_eq!(
            parse(r"C:\한글 폴더\100%.png").unwrap(),
            PathBuf::from(r"C:\한글 폴더\100%.png")
        );
        assert_eq!(
            parse(r"\\server\share\book.cbz").unwrap(),
            PathBuf::from(r"\\server\share\book.cbz")
        );
    }

    #[test]
    fn file_and_smb_uris_decode_unicode_and_spaces() {
        assert_eq!(
            parse("SMB://server/share/%ED%95%9C%EA%B8%80%20book.cbz").unwrap(),
            PathBuf::from(r"\\server\share\한글 book.cbz")
        );
        assert_eq!(
            parse("file:///C:/book%20pages/p1.png").unwrap(),
            PathBuf::from(r"C:\book pages\p1.png")
        );
        assert_eq!(
            parse("file://localhost/C:/a.png").unwrap(),
            PathBuf::from(r"C:\a.png")
        );
        assert_eq!(
            parse("file://server/share/a.png").unwrap(),
            PathBuf::from(r"\\server\share\a.png")
        );
    }

    #[test]
    fn malformed_and_credential_addresses_are_rejected() {
        for uri in [
            "",
            "smb://server",
            "smb:///share",
            "smb://user@server/share",
            "smb://user%40server/share",
            "smb://server/share/%00",
            "smb://server/share/%FF",
            "smb://server/share/%x0",
            "smb://server/share/%",
            "smb://server/share?q=x",
            "file:///relative",
            "ftp://server/book",
            "sftp://server/book",
        ] {
            assert!(parse(uri).is_err(), "{uri}");
        }
    }
}
