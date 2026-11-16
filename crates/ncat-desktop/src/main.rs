//! Non-Linux GTK entry point. Local sources only until OS mount adapters exist.
#![forbid(unsafe_code)]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> gtk::glib::ExitCode {
    ncat_gtk::run(|_, _| Box::pin(async { Err(ncat_backend::NetworkFailure::Unavailable) }))
}
