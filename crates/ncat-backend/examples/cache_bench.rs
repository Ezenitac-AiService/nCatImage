//! Backend-only warm revisit latency; does not measure GTK/compositor presentation.
use ncat_backend::{Backend, BackendOptions, Event, View, worker_path};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: cache_bench PATH")?;
    for cached in [false, true] {
        let backend = Backend::with_options(
            worker_path()?,
            BackendOptions {
                cache_bytes: if cached {
                    BackendOptions::default().cache_bytes
                } else {
                    0
                },
                prefetch: false,
                ..BackendOptions::default()
            },
        )?;
        let events = backend.events();
        backend
            .open(&path, View::default())
            .map_err(|_| "open rejected")?;
        let mut book = None;
        let mut rendered = false;
        while book.is_none() || !rendered {
            match events.recv_blocking()? {
                Event::Catalog { book: next, .. } if next.complete => book = Some(next),
                Event::Frame { .. } => rendered = true,
                Event::Error { code, .. } => return Err(format!("open: {code:?}").into()),
                _ => {}
            }
        }
        let book = book.ok_or("missing catalog")?;
        let mut samples = Vec::new();
        for index in 0..503 {
            let start = Instant::now();
            backend
                .load(book.clone(), index % book.pages.len(), View::default())
                .map_err(|_| "load rejected")?;
            loop {
                match events.recv_blocking()? {
                    Event::Frame { .. } => break,
                    Event::Error { code, .. } => return Err(format!("decode: {code:?}").into()),
                    _ => {}
                }
            }
            if index >= 3 {
                samples.push(start.elapsed().as_micros());
            }
        }
        samples.sort_unstable();
        println!(
            "cached={cached} n={} p50_us={} p95_us={} max_us={} stats={:?}",
            samples.len(),
            samples[249],
            samples[474],
            samples[499],
            backend.cache_stats()
        );
        drop(book);
        backend.shutdown();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !backend.is_closed() {
            if Instant::now() >= deadline {
                return Err("shutdown deadline".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if backend.budget_used() != 0 {
            return Err("reservation leak".into());
        }
    }
    Ok(())
}
