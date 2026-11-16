//! Native Windows viewer and same-executable protected worker. No decoder fallback.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unsafe_code)]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
#[allow(unsafe_code)] // Only the documented Win32 ABI module may cross raw FFI.
mod win32;

#[cfg(any(windows, test))]
mod address;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments == ["--worker"] {
        return ncat_worker::run_protected();
    }
    if arguments == ["--ipc-probe"] {
        return if ipc_probe().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if arguments == ["--self-test"] || arguments == ["--self-test-platform"] {
        return match self_test(arguments == ["--self-test"]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("nCatImage: self-test error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if arguments.len() == 2 && arguments[0] == "--smoke-pipeline" {
        return match pipeline_test(std::path::PathBuf::from(&arguments[1])) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("nCatImage: pipeline test failed ({:?})", error.kind());
                ExitCode::FAILURE
            }
        };
    }
    use win32::Smoke;
    let (initial, smoke) = match arguments.as_slice() {
        [] => (None, Smoke::None),
        [option] if option == "--smoke-window" => (None, Smoke::Window),
        [option, path] if option == "--smoke-frame" => (Some(path.into()), Smoke::Frame),
        [option, path] if option == "--smoke-animation" => (Some(path.into()), Smoke::Animation),
        [option, path] if option == "--smoke-navigation" => (Some(path.into()), Smoke::Navigation),
        [path] if !path.as_encoded_bytes().starts_with(b"--") => (Some(path.into()), Smoke::None),
        _ => return ExitCode::from(2),
    };
    match win32::run(initial, smoke) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nCatImage: native viewer failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn self_test(protected_worker: bool) -> std::io::Result<()> {
    use ncat_protocol::{Envelope, RawPath, Request, Response, read_message, write_message};
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::PathBuf,
    };
    let path = PathBuf::from("C:\\한글 폴더\\😀.png");
    let mut buffer = Vec::new();
    write_message(
        &mut buffer,
        &Envelope::new(
            17,
            Request::Open {
                source: RawPath::new(&path),
            },
        ),
    )?;
    let envelope = read_message::<Request>(&mut buffer.as_slice())?
        .ok_or_else(|| std::io::Error::other("missing IPC"))?;
    match envelope.body {
        Request::Open { source } if envelope.id == 17 && source.path() == path => {}
        _ => return Err(std::io::Error::other("native path IPC")),
    }
    let wide = vec![b'C' as u16, b':' as u16, b'\\' as u16, 0xd800];
    let path = PathBuf::from(OsString::from_wide(&wide));
    if RawPath::new(&path)
        .path()
        .as_os_str()
        .encode_wide()
        .collect::<Vec<_>>()
        != wide
    {
        return Err(std::io::Error::other("native surrogate"));
    }
    let mut playback = ncat_presentation::playback::Playback::default();
    playback.displayed(0, 0, 100);
    playback.set_speed(2.0);
    if playback.delay() != 50 {
        return Err(std::io::Error::other("playback policy"));
    }
    // Start the exact same EXE, with no worker executable or shell on PATH.
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--worker")
        .stdin(std::process::Stdio::null())
        .status()?;
    if status.code() != Some(2) {
        return Err(std::io::Error::other("unprotected worker was not rejected"));
    }
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("--ipc-probe")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let result: std::io::Result<()> = (|| {
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("probe stdin"))?;
        write_message(&mut input, &Envelope::new(19, Request::Shutdown))?;
        drop(input);
        let output = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("probe stdout"))?;
        let reply = read_message::<Response>(&mut std::io::BufReader::new(output))?
            .ok_or_else(|| std::io::Error::other("probe EOF"))?;
        if reply.id != 19 || !matches!(reply.body, Response::Bye) || !child.wait()?.success() {
            return Err(std::io::Error::other("probe reply"));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result?;
    if protected_worker {
        let mut protected = std::process::Command::new(std::env::current_exe()?)
            .arg("--worker")
            .env("NCAT_PROTECTED_WORKER", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let job = ncat_platform::start_protected_worker(&mut protected);
        if let Err(error) = &job {
            let _ = protected.kill();
            let _ = protected.wait();
            return Err(std::io::Error::other(error.to_string()));
        }
        let _job = job?;
        let mut input = protected.stdin.take().unwrap();
        write_message(&mut input, &Envelope::new(23, Request::Shutdown))?;
        drop(input);
        let reply = read_message::<Response>(&mut protected.stdout.take().unwrap())?;
        if !matches!(
            reply,
            Some(Envelope {
                id: 23,
                body: Response::Bye,
                ..
            })
        ) || !protected.wait()?.success()
        {
            let _ = protected.kill();
            let _ = protected.wait();
            return Err(std::io::Error::other("protected worker handshake"));
        }
    }
    let directory = tempfile::tempdir()?;
    let private = directory.path().join("private");
    ncat_platform::create_private_directory(&private)?;
    let file_path = private.join("한글 private pixels");
    let mut file = ncat_platform::create_private_file(&file_path)?;
    use std::io::Write;
    file.write_all(b"pixels")?;
    if ncat_platform::create_private_file(&file_path).is_ok() {
        return Err(std::io::Error::other("create-new contract"));
    }
    let revision =
        ncat_platform::Revision::of(&file).map_err(|_| std::io::Error::other("file identity"))?;
    if revision
        != ncat_platform::Revision::read(&file_path)
            .map_err(|_| std::io::Error::other("path identity"))?
    {
        return Err(std::io::Error::other("file identity mismatch"));
    }
    ncat_platform::open_pixel_output(&file_path)?;
    if file.metadata()?.len() != 0 {
        return Err(std::io::Error::other("pixel truncate"));
    }
    println!("platform_self_test=ok protected_worker={protected_worker}");
    Ok(())
}

#[cfg(windows)]
fn pipeline_test(path: std::path::PathBuf) -> std::io::Result<()> {
    use ncat_backend::{Backend, BackendOptions, Event, View, WorkerCommand};
    let backend = Backend::with_worker_command(
        WorkerCommand::same_executable(std::env::current_exe()?),
        BackendOptions {
            prefetch: false,
            ..BackendOptions::default()
        },
    )?;
    backend
        .open(path, View::default())
        .map_err(|_| std::io::Error::other("open rejected"))?;
    let result = loop {
        match backend.events().recv_blocking() {
            Ok(Event::Frame { frame, .. }) => {
                break if frame.info.valid() && !frame.as_ref().as_ref().is_empty() {
                    Ok(())
                } else {
                    Err(std::io::Error::other("invalid frame"))
                };
            }
            Ok(Event::Error { code, .. }) => {
                eprintln!("nCatImage: pipeline error={code:?}");
                break Err(std::io::Error::other("pipeline worker error"));
            }
            Err(_) => break Err(std::io::Error::other("pipeline event EOF")),
            _ => {}
        }
    };
    backend.shutdown();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !backend.is_closed() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if !backend.is_closed() {
        return Err(std::io::Error::other("worker shutdown deadline"));
    }
    result?;
    println!("pipeline_smoke=ok");
    Ok(())
}

#[cfg(windows)]
fn ipc_probe() -> std::io::Result<()> {
    use ncat_protocol::{Envelope, Request, Response, read_message, write_message};
    let request = read_message::<Request>(&mut std::io::stdin().lock())?
        .ok_or_else(|| std::io::Error::other("probe EOF"))?;
    if !matches!(request.body, Request::Shutdown) {
        return Err(std::io::Error::other("probe only supports shutdown"));
    }
    write_message(
        &mut std::io::stdout().lock(),
        &Envelope::new(request.id, Response::Bye),
    )
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("ncat-windows: native Windows bootstrap; build a Windows target");
    std::process::ExitCode::from(2)
}
