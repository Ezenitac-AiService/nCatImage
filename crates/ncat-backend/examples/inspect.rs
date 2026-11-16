//! Headless M0 smoke tool. Uses exactly the same worker/IPC/budget path as GTK.
use ncat_backend::{Backend, Event, View, worker_path};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: inspect PATH")?;
    let backend = Backend::start(worker_path()?)?;
    let events = backend.events();
    backend
        .open(path, View::default())
        .map_err(|_| "open rejected")?;
    let start = Instant::now();
    let mut success = false;
    while let Ok(event) = events.recv_blocking() {
        match event {
            Event::AnimationEnded { .. } => break,
            Event::Catalog { book, .. } => {
                println!("pages={} complete={}", book.pages.len(), book.complete)
            }
            Event::Frame { frame, .. } => {
                println!(
                    "frame={}x{} original={}x{} bytes={} decode_us={} request_us={} budget_reserved={}",
                    frame.info.width,
                    frame.info.height,
                    frame.info.original_width,
                    frame.info.original_height,
                    frame.info.bytes,
                    frame.info.elapsed_us,
                    start.elapsed().as_micros(),
                    backend.budget_used()
                );
                success = true;
                break;
            }
            Event::Error { code, .. } => {
                eprintln!("{code}");
                break;
            }
        }
    }
    backend.shutdown();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !backend.is_closed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !success || !backend.is_closed() {
        return Err("smoke failed".into());
    }
    Ok(())
}
