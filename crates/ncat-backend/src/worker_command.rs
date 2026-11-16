//! Explicit executable/argument boundaries. Never search PATH or invoke a shell.
use std::{ffi::OsString, path::PathBuf, process::Command};

#[derive(Clone, Debug)]
pub struct WorkerCommand {
    executable: PathBuf,
    arguments: Vec<OsString>,
}

impl WorkerCommand {
    pub fn separate(executable: PathBuf) -> Self {
        Self {
            executable,
            arguments: Vec::new(),
        }
    }

    pub fn same_executable(executable: PathBuf) -> Self {
        Self {
            executable,
            arguments: vec!["--worker".into()],
        }
    }

    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(&self.arguments);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command
                .env("NCAT_PROTECTED_WORKER", "1")
                .creation_flags(0x08000000);
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_exe_mode_does_not_split_spaces_or_interpret_shell_characters() {
        let executable = PathBuf::from("/not-searched/한글 folder/ncat & image.exe");
        let command = WorkerCommand::same_executable(executable.clone()).command();
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("--worker")]
        );
        assert_eq!(
            WorkerCommand::separate(executable)
                .command()
                .get_args()
                .count(),
            0
        );
    }
}
