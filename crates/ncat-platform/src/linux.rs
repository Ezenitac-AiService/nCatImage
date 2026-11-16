// Resource containment, not a filesystem/network sandbox. Never raise inherited limits.
pub fn protect_worker() -> std::io::Result<()> {
    use rustix::process::{self, Pid, Resource, Rlimit, Signal};
    let parent = process::getppid()
        .filter(|p| *p != Pid::INIT)
        .ok_or(rustix::io::Errno::PERM)?;
    let inherited = process::getrlimit(Resource::As);
    let cap = ncat_protocol::WORKER_ADDRESS_SPACE;
    let maximum = inherited.maximum.unwrap_or(cap).min(cap);
    process::setrlimit(
        Resource::As,
        Rlimit {
            current: Some(inherited.current.unwrap_or(maximum).min(maximum)),
            maximum: Some(maximum),
        },
    )?;
    process::setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )?;
    process::set_parent_process_death_signal(Some(Signal::KILL))?;
    rustix::thread::set_no_new_privs(true)?;
    if process::getppid() != Some(parent) {
        return Err(rustix::io::Errno::PERM.into());
    }
    Ok(())
}
