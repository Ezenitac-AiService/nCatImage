//! OS file identity, private files and worker containment.
//! This is not a sandbox. Unsupported services fail closed, never silently weaken protection.
#![forbid(unsafe_code)]

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

#[cfg(not(target_os = "linux"))]
pub fn protect_worker() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "worker containment is not implemented for this OS",
    ))
}

pub fn worker_filename() -> &'static str {
    if cfg!(windows) {
        "ncat-worker.exe"
    } else {
        "ncat-worker"
    }
}
