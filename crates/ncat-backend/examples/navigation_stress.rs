//! Local, path-redacted navigation/memory reproducer using the production backend.
use ncat_backend::{Backend, BackendOptions, Event, Frame, View, worker_path};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: navigation_stress PATH")?;
    let delay_ms: u64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "5".into())
        .parse()?;
    let backend = Backend::with_options(worker_path()?, BackendOptions::from_env()?)?;
    let events = backend.events();
    let view = View::default();
    backend
        .open(path, view)
        .map_err(|e| format!("submit: {e:?}"))?;
    let mut book = None;
    let mut displayed: Option<Arc<Frame>> = None;
    while book.is_none() || displayed.is_none() {
        match events.recv_blocking()? {
            Event::Catalog { book: next, .. } if next.complete => book = Some(next),
            Event::Frame { frame, .. } => displayed = Some(frame),
            Event::Error { code, .. } => return Err(format!("open: {code:?}").into()),
            _ => {}
        }
    }
    let book = book.ok_or("catalog missing")?;
    println!(
        "pages={} budget_limit={}",
        book.pages.len(),
        BackendOptions::from_env()?.budget
    );
    let mut errors = 0;
    let mut peak_reserved = backend.budget_used();
    for round in 0..100 {
        for offset in 0..5 {
            let index = ((round * 5 + offset) * 17) % book.pages.len();
            backend
                .load(book.clone(), index, view)
                .map_err(|e| format!("submit: {e:?}"))?;
            std::thread::sleep(Duration::from_millis(delay_ms));
            peak_reserved = peak_reserved.max(backend.budget_used());
        }
        loop {
            let event = events.recv_blocking()?;
            if event.generation() != backend.generation() {
                continue;
            }
            match event {
                Event::Frame { frame, .. } => {
                    displayed = Some(frame);
                    break;
                }
                Event::Error { code, .. } => {
                    errors += 1;
                    println!("round={round} error={code:?}");
                    break;
                }
                _ => {}
            }
        }
        peak_reserved = peak_reserved.max(backend.budget_used());
    }
    println!(
        "errors={errors} sampled_peak_reserved={peak_reserved} stats={:?}",
        backend.cache_stats()
    );
    drop((book, displayed));
    backend.shutdown();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !backend.is_closed() {
        if Instant::now() >= deadline {
            return Err("shutdown deadline".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("remaining_reserved={}", backend.budget_used());
    if errors > 0 {
        return Err("navigation failed".into());
    }
    Ok(())
}
