//! OS file identity, private files and worker containment.
//! This is not a sandbox. Unsupported services fail closed, never silently weaken protection.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
pub use windows::{
    Revision, WorkerProtection, create_private_directory, create_private_file, name_bytes,
    open_pixel_output, protect_worker, start_protected_worker,
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{
    Revision, create_private_directory, create_private_file, name_bytes, open_pixel_output,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::protect_worker;

#[cfg(not(any(target_os = "linux", windows)))]
pub fn protect_worker() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "worker containment is not implemented for this OS",
    ))
}

#[cfg(unix)]
pub struct WorkerProtection;
#[cfg(unix)]
pub fn start_protected_worker(_: &mut std::process::Child) -> std::io::Result<WorkerProtection> {
    Ok(WorkerProtection)
}
pub fn path_sort_bytes(path: &ncat_protocol::RawPath) -> std::borrow::Cow<'_, [u8]> {
    #[cfg(unix)]
    {
        std::borrow::Cow::Borrowed(&path.0)
    }
    #[cfg(windows)]
    {
        std::borrow::Cow::Owned(path.path().as_os_str().as_encoded_bytes().to_vec())
    }
}

pub fn worker_filename() -> &'static str {
    if cfg!(windows) {
        "ncat-worker.exe"
    } else {
        "ncat-worker"
    }
}
