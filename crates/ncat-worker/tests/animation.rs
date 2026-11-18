#![cfg(unix)] // Standalone Unix worker; native Windows CI tests the same EXE.
use image::{ImageBuffer, ImageEncoder, Rgba};
use ncat_backend::{Backend, BackendOptions, Book, Event, Frame, View};
use ncat_protocol::ErrorCode;
use std::{
    fs::{self, File},
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn backend() -> Backend {
    Backend::with_options(
        env!("CARGO_BIN_EXE_ncat-worker").into(),
        BackendOptions {
            prefetch: true,
            ..BackendOptions::default()
        },
    )
    .unwrap()
}

fn event(backend: &Backend) -> Event {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(event) = backend.events().try_recv()
            && event.generation() == backend.generation()
        {
            return event;
        }
        assert!(Instant::now() < deadline, "animation event timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn open(backend: &Backend, path: &Path, view: View) -> (Arc<Book>, Arc<Frame>) {
    backend.open(path, view).unwrap();
    let (mut book, mut frame) = (None, None);
    while book.is_none() || frame.is_none() {
        match event(backend) {
            Event::Catalog { book: value, .. } if value.complete => book = Some(value),
            Event::Frame { frame: value, .. } => frame = Some(value),
            Event::Error { code, .. } => panic!("animation open: {code:?}"),
            _ => {}
        }
    }
    (book.unwrap(), frame.unwrap())
}

fn next(backend: &Backend, book: &Arc<Book>, view: View) -> Arc<Frame> {
    backend
        .animation_next(book.clone(), book.selected, view)
        .unwrap();
    match event(backend) {
        Event::Frame {
            frame,
            cached: false,
            ..
        } => frame,
        other => panic!("expected sequential frame, got {other:?}"),
    }
}

fn close(backend: &Backend) {
    backend.shutdown();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !backend.is_closed() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn gif(path: &Path, repeat: Option<gif::Repeat>) {
    let mut encoder = gif::Encoder::new(File::create(path).unwrap(), 2, 1, &[]).unwrap();
    if let Some(repeat) = repeat {
        encoder.set_repeat(repeat).unwrap();
    }
    for (color, delay) in [([255, 0, 0, 255], 3), ([0, 0, 255, 255], 7)] {
        let mut pixels = color.repeat(2);
        let mut frame = gif::Frame::from_rgba_speed(2, 1, &mut pixels, 10);
        frame.delay = delay;
        encoder.write_frame(&frame).unwrap();
    }
}

fn apng(path: &Path, plays: u32, separate_default: bool) {
    let mut encoder = png::Encoder::new(File::create(path).unwrap(), 2, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(2, plays).unwrap();
    encoder.set_sep_def_img(separate_default).unwrap();
    encoder
        .add_text_chunk("Title".into(), "APNG metadata fixture".into())
        .unwrap();
    let mut writer = encoder.write_header().unwrap();
    if separate_default {
        writer
            .write_image_data(&[0, 255, 0, 255].repeat(2))
            .unwrap();
    }
    for (color, delay) in [([255, 0, 0, 255], 3), ([0, 0, 255, 255], 7)] {
        writer.set_frame_delay(delay, 100).unwrap();
        writer.write_image_data(&color.repeat(2)).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn embedded_metadata_survives_worker_frames_seek_and_clears_on_plain_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("animation.png");
    apng(&path, 0, false);
    let original = fs::read(&path).unwrap();
    let backend = backend();
    let view = View::default();
    let (book, first) = open(&backend, &path, view);
    assert_eq!(first.info.metadata[0].value, "APNG metadata fixture");
    let second = next(&backend, &book, view);
    assert_eq!(second.info.metadata, first.info.metadata);
    backend
        .animation_seek(book.clone(), book.selected, view, 0)
        .unwrap();
    let Event::Frame { frame: sought, .. } = event(&backend) else {
        panic!("expected seek frame")
    };
    assert_eq!(sought.info.metadata, first.info.metadata);
    let plain = dir.path().join("plain.png");
    image::RgbImage::new(4, 4).save(&plain).unwrap();
    let (_, plain) = open(&backend, &plain, view);
    assert!(plain.info.metadata.is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
    drop((first, second, sought, plain));
    close(&backend);
}

fn chunk(target: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
    target.extend(kind);
    target.extend((bytes.len() as u32).to_le_bytes());
    target.extend(bytes);
    if !bytes.len().is_multiple_of(2) {
        target.push(0);
    }
}

fn webp(path: &Path, plays: u16) {
    let mut body = Vec::new();
    // VP8X: animation and alpha; 2x1 canvas.
    chunk(&mut body, b"VP8X", &[0x12, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    let mut anim = vec![0, 0, 0, 0];
    anim.extend(plays.to_le_bytes());
    chunk(&mut body, b"ANIM", &anim);
    for (color, delay) in [([255, 0, 0, 255], 30_u8), ([0, 0, 255, 255], 70)] {
        let mut encoded = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .write_image(&color.repeat(2), 2, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let mut frame = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, delay, 0, 0, 2];
        frame.extend(&encoded[12..]); // Complete VP8L subchunk, no nested RIFF header.
        chunk(&mut body, b"ANMF", &frame);
    }
    let mut riff = b"RIFF".to_vec();
    riff.extend((body.len() as u32 + 4).to_le_bytes());
    riff.extend(b"WEBP");
    riff.extend(body);
    fs::write(path, riff).unwrap();
}

#[test]
fn gif_webp_apng_play_all_frames_with_delays_and_finite_loops() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["agif", "webp", "apng", "png"] {
        let path = root.path().join(format!("animation.{kind}"));
        match kind {
            "agif" => gif(&path, Some(gif::Repeat::Finite(1))),
            "webp" => webp(&path, 2),
            _ => apng(&path, 2, kind == "png"),
        }
        let original = fs::read(&path).unwrap();
        let backend = backend();
        let view = View::default();
        let (book, first) = open(&backend, &path, view);
        assert_eq!(&first.as_ref().as_ref()[..4], &[255, 0, 0, 255], "{kind}");
        assert_eq!(first.info.animation.as_ref().unwrap().delay_ms, 30);
        drop(first);
        for (index, cycle, color, delay) in [
            (1, 0, [0, 0, 255, 255], 70),
            (0, 1, [255, 0, 0, 255], 30),
            (1, 1, [0, 0, 255, 255], 70),
        ] {
            let frame = next(&backend, &book, view);
            let animation = frame.info.animation.as_ref().unwrap();
            assert_eq!(
                (animation.index, animation.cycle, animation.delay_ms),
                (index, cycle, delay),
                "{kind}"
            );
            assert_eq!(&frame.as_ref().as_ref()[..4], &color);
        }
        backend
            .animation_next(book.clone(), book.selected, view)
            .unwrap();
        assert!(
            matches!(event(&backend), Event::AnimationEnded { .. }),
            "{kind}"
        );
        assert_eq!(backend.cache_stats().prefetches, 0);
        assert_eq!(fs::read(&path).unwrap(), original);
        close(&backend);
        drop(book);
        assert_eq!(backend.budget_used(), 0);
    }
}

#[test]
fn infinite_stream_is_bounded_and_cancel_releases_decoder_reservation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("infinite.gif");
    gif(&path, Some(gif::Repeat::Infinite));
    let backend = backend();
    let view = View::default();
    let (book, first) = open(&backend, &path, view);
    drop(first);
    let baseline = backend.budget_used();
    for _ in 0..100 {
        let frame = next(&backend, &book, view);
        assert!(frame.info.animation.is_some());
        drop(frame);
        assert_eq!(backend.budget_used(), baseline);
    }
    backend.cancel();
    let deadline = Instant::now() + Duration::from_secs(2);
    while backend.budget_used() == baseline {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    close(&backend);
    drop(book);
    assert_eq!(backend.budget_used(), 0);
}

#[test]
fn gif_without_loop_extension_stops_after_one_play() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("once.gif");
    gif(&path, None);
    let backend = backend();
    let view = View::default();
    let (book, first) = open(&backend, &path, view);
    drop(first);
    drop(next(&backend, &book, view));
    backend
        .animation_next(book.clone(), book.selected, view)
        .unwrap();
    assert!(matches!(event(&backend), Event::AnimationEnded { .. }));
    close(&backend);
}

#[test]
fn zip_animation_rotation_and_source_change_keep_worker_boundary() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.apng");
    apng(&source, 0, false);
    let path = root.path().join("book.cbz");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("page.apng", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&fs::read(source).unwrap()).unwrap();
    fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();
    let backend = backend();
    let view = View {
        quarter_turns: 1,
        ..View::default()
    };
    let (book, first) = open(&backend, &path, view);
    assert_eq!((first.info.width, first.info.height), (1, 2));
    drop(first);
    drop(next(&backend, &book, view));
    fs::write(&path, b"changed").unwrap();
    backend
        .animation_next(book.clone(), book.selected, view)
        .unwrap();
    assert!(matches!(
        event(&backend),
        Event::Error {
            code: ErrorCode::Changed,
            ..
        }
    ));
    close(&backend);
}

#[test]
fn selecting_static_page_stops_animation_and_old_pixels_remain_owned() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("1.gif");
    gif(&path, Some(gif::Repeat::Infinite));
    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_pixel(2, 1, Rgba([0, 255, 0, 255]))
        .save(root.path().join("2.png"))
        .unwrap();
    let backend = backend();
    let view = View::default();
    let (book, old) = open(&backend, &path, view);
    backend.load(book, 1, view).unwrap();
    match event(&backend) {
        Event::Frame { frame, .. } => {
            assert!(frame.info.animation.is_none());
            assert_eq!(&frame.as_ref().as_ref()[..4], &[0, 255, 0, 255]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(&old.as_ref().as_ref()[..4], &[255, 0, 0, 255]);
    close(&backend);
    assert!(backend.budget_used() > 0);
    drop(old);
    assert_eq!(backend.budget_used(), 0);
}

#[test]
fn gif_and_apng_restore_previous_canvas_for_partial_frames() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["gif", "apng"] {
        let path = root.path().join(format!("partial.{kind}"));
        if kind == "gif" {
            let mut encoder = gif::Encoder::new(File::create(&path).unwrap(), 2, 1, &[]).unwrap();
            for (width, left, color, disposal) in [
                (2, 0, [255, 0, 0, 255], gif::DisposalMethod::Keep),
                (1, 0, [0, 255, 0, 255], gif::DisposalMethod::Previous),
                (1, 1, [0, 0, 255, 255], gif::DisposalMethod::Keep),
            ] {
                let mut pixels = color.repeat(width as usize);
                let mut frame = gif::Frame::from_rgba_speed(width, 1, &mut pixels, 10);
                frame.left = left;
                frame.dispose = disposal;
                encoder.write_frame(&frame).unwrap();
            }
        } else {
            let mut encoder = png::Encoder::new(File::create(&path).unwrap(), 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(3, 1).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255].repeat(2))
                .unwrap();
            writer.set_frame_dimension(1, 1).unwrap();
            writer.set_dispose_op(png::DisposeOp::Previous).unwrap();
            writer.write_image_data(&[0, 255, 0, 255]).unwrap();
            writer.set_frame_position(1, 0).unwrap();
            writer.set_dispose_op(png::DisposeOp::None).unwrap();
            writer.write_image_data(&[0, 0, 255, 255]).unwrap();
            writer.finish().unwrap();
        }
        let backend = backend();
        let view = View::default();
        let (book, first) = open(&backend, &path, view);
        drop(first);
        let green = next(&backend, &book, view);
        assert_eq!(
            green.as_ref().as_ref(),
            &[0, 255, 0, 255, 255, 0, 0, 255],
            "{kind}"
        );
        let blue = next(&backend, &book, view);
        assert_eq!(
            blue.as_ref().as_ref(),
            &[255, 0, 0, 255, 0, 0, 255, 255],
            "{kind}"
        );
        close(&backend);
    }
}

#[test]
fn webp_partial_frames_dispose_to_transparent_background() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("partial.webp");
    let mut body = Vec::new();
    chunk(&mut body, b"VP8X", &[0x12, 0, 0, 0, 3, 0, 0, 0, 0, 0]);
    chunk(&mut body, b"ANIM", &[0, 0, 0, 0, 1, 0]);
    for (width, left_half, color, flags) in [
        (4, 0, [255, 0, 0, 255], 2),
        (2, 0, [0, 255, 0, 255], 3),
        (2, 1, [0, 0, 255, 255], 2),
    ] {
        let mut encoded = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .write_image(
                &color.repeat(width as usize),
                width,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut frame = vec![
            left_half,
            0,
            0,
            0,
            0,
            0,
            (width - 1) as u8,
            0,
            0,
            0,
            0,
            0,
            30,
            0,
            0,
            flags,
        ];
        frame.extend(&encoded[12..]);
        chunk(&mut body, b"ANMF", &frame);
    }
    let mut riff = b"RIFF".to_vec();
    riff.extend((body.len() as u32 + 4).to_le_bytes());
    riff.extend(b"WEBP");
    riff.extend(body);
    fs::write(&path, riff).unwrap();
    let backend = backend();
    let view = View::default();
    let (book, first) = open(&backend, &path, view);
    drop(first);
    let green = next(&backend, &book, view);
    assert_eq!(
        green.as_ref().as_ref(),
        &[[0, 255, 0, 255].repeat(2), [255, 0, 0, 255].repeat(2)].concat()
    );
    let blue = next(&backend, &book, view);
    assert_eq!(
        blue.as_ref().as_ref(),
        &[[0, 0, 0, 0].repeat(2), [0, 0, 255, 255].repeat(2)].concat()
    );
    close(&backend);
}

#[test]
fn animation_admission_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("animation.apng");
    apng(&path, 0, false);
    let backend = Backend::with_options(
        env!("CARGO_BIN_EXE_ncat-worker").into(),
        BackendOptions {
            workspace_bytes: ncat_protocol::DECODE_OVERHEAD,
            prefetch: false,
            ..BackendOptions::default()
        },
    )
    .unwrap();
    backend.open(&path, View::default()).unwrap();
    loop {
        match event(&backend) {
            Event::Error {
                code: ErrorCode::Limit,
                ..
            } => break,
            Event::Catalog { .. } => {}
            other => panic!("expected budget rejection, got {other:?}"),
        }
    }
    close(&backend);
    assert_eq!(backend.budget_used(), 0);
}

#[test]
fn corrupt_later_frame_stops_stream_and_next_source_still_opens() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("broken.apng");
    apng(&path, 0, false);
    let mut bytes = fs::read(&path).unwrap();
    let offset = bytes
        .windows(4)
        .rposition(|chunk| chunk == b"fdAT")
        .unwrap();
    bytes[offset + 8] ^= 0xff; // Damage compressed second-frame data/CRC before opening.
    fs::write(&path, bytes).unwrap();
    let backend = backend();
    let view = View::default();
    let (book, first) = open(&backend, &path, view);
    drop(first);
    backend
        .animation_next(book.clone(), book.selected, view)
        .unwrap();
    assert!(matches!(
        event(&backend),
        Event::Error {
            code: ErrorCode::Corrupt,
            ..
        }
    ));
    let clean = root.path().join("clean.png");
    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_pixel(2, 1, Rgba([0, 255, 0, 255]))
        .save(&clean)
        .unwrap();
    let (new_book, frame) = open(&backend, &clean, view);
    assert!(frame.info.animation.is_none());
    close(&backend);
    drop((book, new_book, frame));
    assert_eq!(backend.budget_used(), 0);
}

#[test]
fn seeking_previous_frame_and_restarting_after_finite_end_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["gif", "apng", "webp"] {
        let path = root.path().join(format!("seek.{kind}"));
        match kind {
            "gif" => gif(&path, None),
            "apng" => apng(&path, 1, false),
            _ => webp(&path, 1),
        }
        let backend = backend();
        let view = View::default();
        let (book, first) = open(&backend, &path, view);
        drop(first);
        let baseline = backend.budget_used();
        drop(next(&backend, &book, view));
        for _ in 0..4 {
            backend
                .animation_seek(book.clone(), book.selected, view, 0)
                .unwrap();
            match event(&backend) {
                Event::Frame { frame, .. } => {
                    assert_eq!(frame.info.animation.as_ref().unwrap().index, 0);
                    assert_eq!(&frame.as_ref().as_ref()[..4], &[255, 0, 0, 255]);
                }
                other => panic!("{other:?}"),
            }
            drop(next(&backend, &book, view));
            backend
                .animation_next(book.clone(), book.selected, view)
                .unwrap();
            assert!(matches!(event(&backend), Event::AnimationEnded { .. }));
            assert_eq!(backend.budget_used(), baseline);
        }
        close(&backend);
        drop(book);
        assert_eq!(backend.budget_used(), 0);
    }
}

#[test]
fn unknown_frame_count_seek_past_end_clamps_without_wrapping_or_error() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["gif", "apng", "webp"] {
        let path = root.path().join(format!("jump.{kind}"));
        match kind {
            "gif" => gif(&path, Some(gif::Repeat::Infinite)),
            "apng" => apng(&path, 0, false),
            _ => webp(&path, 0),
        }
        let backend = backend();
        let view = View::default();
        let (book, first) = open(&backend, &path, view);
        assert_eq!(first.info.animation.as_ref().unwrap().total, None);
        drop(first);
        backend
            .animation_seek(book.clone(), book.selected, view, 9999)
            .unwrap();
        match event(&backend) {
            Event::Frame { frame, .. } => {
                let animation = frame.info.animation.as_ref().unwrap();
                assert_eq!((animation.index, animation.total), (1, Some(2)));
                assert_eq!(&frame.as_ref().as_ref()[..4], &[0, 0, 255, 255]);
            }
            other => panic!("jump past EOF must keep a valid image: {other:?}"),
        }
        let resumed = next(&backend, &book, view);
        assert_eq!(resumed.info.animation.as_ref().unwrap().index, 0);
        close(&backend);
        drop((resumed, book));
        assert_eq!(backend.budget_used(), 0);
    }
}
