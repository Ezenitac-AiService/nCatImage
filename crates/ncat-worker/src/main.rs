#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    ncat_worker::run_protected()
}
