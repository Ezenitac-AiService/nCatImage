//! Best-effort embedded metadata, parsed ONLY in the worker. All scans, payload
//! allocations, decompression and IPC text are bounded; malformed metadata does
//! not turn a decodable image into an error. No XML/markup execution or logging.
use ncat_protocol::{MAX_METADATA_BYTES, MAX_METADATA_FIELDS, MAX_METADATA_VALUE, MetadataEntry};
use std::io::{Read, Seek, SeekFrom};

const SCAN_BYTES: usize = 256 * 1024;
const PAYLOAD: usize = 64 * 1024;

#[derive(Default)]
struct Text {
    fields: Vec<MetadataEntry>,
    bytes: usize,
}
fn clean(value: &str, max: usize) -> String {
    let mut result = String::new();
    let mut truncated = false;
    for c in value.chars().filter(|c| {
        (!c.is_control() || matches!(c, '\n' | '\t'))
            && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        if result.len() + c.len_utf8() > max {
            truncated = true;
            break;
        }
        result.push(c);
    }
    if truncated && max >= 3 {
        while result.len() + '…'.len_utf8() > max {
            result.pop();
        }
        result.push('…');
    }
    result.trim().to_owned()
}
impl Text {
    fn full(&self) -> bool {
        self.fields.len() >= MAX_METADATA_FIELDS || self.bytes + 256 >= MAX_METADATA_BYTES
    }
    fn push(&mut self, name: &str, value: &str) {
        if self.full() {
            return;
        }
        let name = clean(name, 128);
        let limit = MAX_METADATA_BYTES
            .saturating_sub(self.bytes + name.len() + 64)
            .min(MAX_METADATA_VALUE);
        let value = clean(value, limit);
        let bytes = name.len() + value.len() + 64;
        if !name.is_empty()
            && !value.is_empty()
            && self.fields.len() < MAX_METADATA_FIELDS
            && self.bytes + bytes <= MAX_METADATA_BYTES
        {
            self.bytes += bytes;
            self.fields.push(MetadataEntry { name, value });
        }
    }
    fn exif(&mut self, bytes: &[u8]) {
        let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
        if let Ok(exif) = exif::Reader::new().read_raw(bytes.to_vec()) {
            for field in exif.fields().take(MAX_METADATA_FIELDS) {
                if self.full() {
                    break;
                }
                self.push(
                    &format!("EXIF {}", field.tag),
                    &field.display_value().with_unit(&exif).to_string(),
                );
            }
        }
    }
}
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}
fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .take((PAYLOAD + 1) as u64)
        .read_to_end(&mut output)
        .ok()?;
    (output.len() <= PAYLOAD).then_some(output)
}
fn zero(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = bytes.iter().position(|b| *b == 0)?;
    Some((&bytes[..at], &bytes[at + 1..]))
}
fn png_text(text: &mut Text, kind: &[u8], bytes: &[u8]) {
    let Some((key, value)) = zero(bytes) else {
        return;
    };
    let name = latin1(key);
    match kind {
        b"tEXt" => text.push(&name, &latin1(value)),
        b"zTXt" if value.first() == Some(&0) => {
            if let Some(value) = inflate(&value[1..]) {
                text.push(&name, &latin1(&value));
            }
        }
        b"iTXt" if value.len() >= 2 && value[1] == 0 => {
            let Some((_, rest)) = zero(&value[2..]) else {
                return;
            };
            let Some((_, rest)) = zero(rest) else {
                return;
            };
            match value[0] {
                0 => text.push(&name, &String::from_utf8_lossy(rest)),
                1 => {
                    if let Some(bytes) = inflate(rest) {
                        text.push(&name, &String::from_utf8_lossy(&bytes));
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

struct Scan<'a, R: ?Sized> {
    input: &'a mut R,
    remaining: usize,
}
impl<R: Read + Seek + ?Sized> Scan<'_, R> {
    fn read(&mut self, size: usize) -> Option<Vec<u8>> {
        if size > self.remaining || size > PAYLOAD {
            return None;
        }
        self.remaining -= size;
        let mut bytes = vec![0; size];
        self.input.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }
    fn skip(&mut self, size: u64) -> Option<()> {
        self.input
            .seek(SeekFrom::Current(i64::try_from(size).ok()?))
            .ok()?;
        Some(())
    }
    fn png(&mut self, text: &mut Text) -> Option<()> {
        for _ in 0..4096 {
            let header = self.read(8)?;
            let len = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
            let kind = &header[4..];
            if kind == b"IEND" {
                break;
            }
            if matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf") && len <= PAYLOAD {
                let payload = self.read(len)?;
                if kind == b"eXIf" {
                    text.exif(&payload);
                } else {
                    png_text(text, kind, &payload);
                }
            } else {
                self.skip(len as u64)?;
            }
            self.skip(4)?; // CRC belongs to the image decoder, not to text rendering.
            if text.full() {
                break;
            }
        }
        Some(())
    }
    fn jpeg(&mut self, text: &mut Text) -> Option<()> {
        for _ in 0..4096 {
            if self.read(1)? != [0xff] {
                break;
            }
            let mut marker = self.read(1)?[0];
            while marker == 0xff {
                marker = self.read(1)?[0];
            }
            if matches!(marker, 0xda | 0xd9) {
                break;
            }
            if matches!(marker, 0x01 | 0xd0..=0xd8) {
                continue;
            }
            let length = self.read(2)?;
            let len = usize::from(u16::from_be_bytes(length.try_into().ok()?)).checked_sub(2)?;
            if matches!(marker, 0xe1 | 0xfe) {
                let payload = self.read(len)?;
                if marker == 0xfe {
                    text.push("JPEG Comment", &String::from_utf8_lossy(&payload));
                } else if payload.starts_with(b"Exif\0\0") {
                    text.exif(&payload);
                } else if let Some(xmp) = payload.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                    text.push("XMP", &String::from_utf8_lossy(xmp));
                }
            } else {
                self.skip(len as u64)?;
            }
            if text.full() {
                break;
            }
        }
        Some(())
    }
    fn webp(&mut self, text: &mut Text, end: u64) -> Option<()> {
        for _ in 0..4096 {
            let position = self.input.stream_position().ok()?;
            if position.checked_add(8)? > end {
                break;
            }
            let header = self.read(8)?;
            let len = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
            if position.checked_add(8 + len as u64 + (len % 2) as u64)? > end {
                break;
            }
            if matches!(&header[..4], b"EXIF" | b"XMP ") && len <= PAYLOAD {
                let bytes = self.read(len)?;
                if &header[..4] == b"EXIF" {
                    text.exif(&bytes);
                } else {
                    text.push("XMP", &String::from_utf8_lossy(&bytes));
                }
            } else {
                self.skip(len as u64)?;
            }
            self.skip((len % 2) as u64)?;
            if text.full() {
                break;
            }
        }
        Some(())
    }
    fn gif_blocks(&mut self, collect: bool) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        loop {
            let len = usize::from(self.read(1)?[0]);
            if len == 0 {
                return Some(value);
            }
            if collect && value.len() + len <= PAYLOAD {
                value.extend(self.read(len)?);
            } else {
                self.skip(len as u64)?;
            }
        }
    }
    fn gif(&mut self, text: &mut Text) -> Option<()> {
        self.input.seek(SeekFrom::Start(0)).ok()?;
        let header = self.read(13)?;
        if header[10] & 0x80 != 0 {
            self.skip(3 * (1u64 << ((header[10] & 7) + 1)))?;
        }
        for _ in 0..4096 {
            match self.read(1)?[0] {
                0x21 => {
                    let comment = self.read(1)?[0] == 0xfe;
                    let bytes = self.gif_blocks(comment)?;
                    if comment {
                        text.push("GIF Comment", &String::from_utf8_lossy(&bytes));
                    }
                }
                0x2c => {
                    let descriptor = self.read(9)?;
                    if descriptor[8] & 0x80 != 0 {
                        self.skip(3 * (1u64 << ((descriptor[8] & 7) + 1)))?;
                    }
                    self.read(1)?; // LZW minimum code size, never decode pixels here.
                    self.gif_blocks(false)?;
                }
                _ => break,
            }
            if text.full() {
                break;
            }
        }
        Some(())
    }
}

pub(super) fn extract(input: &mut (impl Read + Seek + ?Sized)) -> Vec<MetadataEntry> {
    let mut text = Text::default();
    let mut scan = Scan {
        input,
        remaining: SCAN_BYTES,
    };
    if scan.input.seek(SeekFrom::Start(0)).is_ok()
        && let Some(signature) = scan.read(8)
    {
        if signature == b"\x89PNG\r\n\x1a\n" {
            scan.png(&mut text);
        } else if signature.starts_with(b"GIF87a") || signature.starts_with(b"GIF89a") {
            scan.gif(&mut text);
        } else if signature.starts_with(b"\xff\xd8") {
            if scan.input.seek(SeekFrom::Start(2)).is_ok() {
                scan.jpeg(&mut text);
            }
        } else if &signature[..4] == b"RIFF"
            && let Some(kind) = scan.read(4)
            && kind == b"WEBP"
        {
            let end = u64::from(u32::from_le_bytes(signature[4..].try_into().unwrap())) + 8;
            scan.webp(&mut text, end);
        }
    }
    text.fields
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    pub fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(data);
        let mut crc = !0u32;
        for byte in &out[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        out.extend((!crc).to_be_bytes());
        out
    }
    fn compressed(text: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(text).unwrap();
        encoder.finish().unwrap()
    }
    #[test]
    fn png_plain_compressed_utf8_after_pixels_and_unsafe_text() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IDAT", b"skipped pixels"));
        png.extend(chunk(b"tEXt", b"Title\0<b>plain</b>\x1b"));
        let mut ztxt = b"Comment\0\0".to_vec();
        ztxt.extend(compressed(b"zipped"));
        png.extend(chunk(b"zTXt", &ztxt));
        png.extend(chunk(
            b"iTXt",
            "parameters\0\0\0\0\0한글 prompt\nseed=42".as_bytes(),
        ));
        png.extend(chunk(b"IEND", &[]));
        let fields = extract(&mut Cursor::new(png));
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].value, "<b>plain</b>");
        assert_eq!(fields[1].value, "zipped");
        assert_eq!(fields[2].value, "한글 prompt\nseed=42");
    }
    #[test]
    fn malformed_oversized_and_compression_bombs_are_optional() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ztxt = b"bomb\0\0".to_vec();
        ztxt.extend(compressed(&vec![b'x'; PAYLOAD + 1000]));
        png.extend(chunk(b"zTXt", &ztxt));
        png.extend(chunk(b"tEXt", b"valid\0still here"));
        for _ in 0..100 {
            png.extend(chunk(b"tEXt", b"a\0value"));
        }
        let fields = extract(&mut Cursor::new(png));
        assert_eq!(fields[0].name, "valid");
        assert_eq!(fields.len(), MAX_METADATA_FIELDS);
        for input in [
            vec![],
            vec![0xff; 100],
            b"\x89PNG\r\n\x1a\n\xff\xff\xff\xfftEXt".to_vec(),
        ] {
            assert!(extract(&mut Cursor::new(input)).is_empty());
        }
        let mut text = Text::default();
        text.push("safe", &format!("\u{202e}{}\0", "한".repeat(10000)));
        assert!(text.fields[0].value.len() <= MAX_METADATA_VALUE);
        assert!(ncat_protocol::metadata_text_safe(&text.fields[0].value));
    }
    #[test]
    fn jpeg_exif_and_comment_and_webp_xmp() {
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut jpeg = b"\xff\xd8\xff\xe1".to_vec();
        jpeg.extend(((exif.len() + 2) as u16).to_be_bytes());
        jpeg.extend(exif);
        jpeg.extend(b"\xff\xfe\0\x07hello\xff\xd9");
        let fields = extract(&mut Cursor::new(jpeg));
        assert_eq!(fields.len(), 2);
        assert!(fields[0].name.contains("Orientation"));
        assert_eq!(fields[1].value, "hello");
        let mut webp = b"RIFF\x10\0\0\0WEBPXMP \x04\0\0\0text".to_vec();
        assert_eq!(extract(&mut Cursor::new(&mut webp))[0].value, "text");
    }
    #[test]
    fn png_metadata_survives_decode_and_zip_without_source_changes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("page.png");
        image::RgbImage::new(4, 3).save(&path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let text = chunk(b"tEXt", b"Title\0worker metadata fixture");
        bytes.splice(33..33, text);
        std::fs::write(&path, &bytes).unwrap();
        let private = root.path().join("private");
        ncat_platform::create_private_directory(&private).unwrap();
        let output = private.join("pixels");
        let _owner = ncat_platform::create_private_file(&output).unwrap();
        let book = crate::Book::open(&path).unwrap();
        let info = book
            .decode(&book.pages[0].key, &output, 100, 100, 32 * 1024 * 1024, 0)
            .unwrap();
        assert!(info.valid());
        assert_eq!(info.metadata[0].value, "worker metadata fixture");
        let archive_path = root.path().join("book.cbz");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        archive
            .start_file("page.png", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
        archive.finish().unwrap();
        let book = crate::Book::open(&archive_path).unwrap();
        let info = book
            .decode(&book.pages[0].key, &output, 100, 100, 32 * 1024 * 1024, 1)
            .unwrap();
        assert_eq!(info.metadata[0].value, "worker metadata fixture");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
