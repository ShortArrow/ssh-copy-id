//! The real `Ssh` backend: runs the system `ssh` client as a child process.

use crate::app::{Ssh, SshOutput};
use crate::platform;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Runs `ssh` found on `PATH`, or the program given to `SystemSsh::with_program`,
/// and `ssh-add` found on `PATH`.
pub struct SystemSsh {
    program: OsString,
    agent_program: OsString,
}

impl SystemSsh {
    /// Uses `ssh` from `PATH`, as upstream does.
    pub fn new() -> SystemSsh {
        SystemSsh::with_program("ssh")
    }

    /// Uses the given program instead of `ssh` from `PATH`.
    pub fn with_program(program: impl Into<OsString>) -> SystemSsh {
        SystemSsh {
            program: program.into(),
            agent_program: OsString::from("ssh-add"),
        }
    }
}

impl Default for SystemSsh {
    fn default() -> SystemSsh {
        SystemSsh::new()
    }
}

impl Ssh for SystemSsh {
    /// Runs `ssh` with `stdin` as its standard input.
    fn run(
        &mut self,
        args: &[String],
        stdin: &[u8],
        capture_stderr: bool,
    ) -> io::Result<SshOutput> {
        let mut command = Command::new(&self.program);
        command.args(args);
        run_through_files(command, stdin, capture_stderr)
    }

    /// Runs `ssh-add -L` with empty input, as upstream's `ssh-add -L` reads nothing.
    fn list_agent_keys(&mut self) -> io::Result<SshOutput> {
        let mut command = Command::new(&self.agent_program);
        command.arg("-L");
        run_through_files(command, b"", true)
    }
}

/// Runs `command` to completion with its standard streams on files in a new
/// private directory, never on pipes: Windows OpenSSH can hang reading or
/// writing a pipe when it runs without a console
/// (PowerShell/Win32-OpenSSH#1330, #1427).
///
/// Standard input holds `stdin`. Uncaptured stderr is inherited when it is a
/// terminal, so that messages appear as they are written; otherwise it is
/// copied to this process's stderr once the command exits, and
/// `SshOutput::stderr` is empty.
fn run_through_files(
    mut command: Command,
    stdin: &[u8],
    capture_stderr: bool,
) -> io::Result<SshOutput> {
    let dir = StreamDir::create()?;
    let input = dir.file("stdin");
    let output = dir.file("stdout");
    let errors = dir.file("stderr");
    fs::write(&input, stdin)?;
    let to_file = capture_stderr || !io::stderr().is_terminal();
    let status = command
        .stdin(File::open(&input)?)
        .stdout(create(&output)?)
        .stderr(if to_file {
            Stdio::from(create(&errors)?)
        } else {
            Stdio::inherit()
        })
        .status()?;
    let mut stderr = if to_file {
        fs::read(&errors)?
    } else {
        Vec::new()
    };
    if !capture_stderr {
        io::stderr().write_all(&stderr)?;
        stderr.clear();
    }
    Ok(SshOutput {
        status: status.code(),
        stdout: fs::read(&output)?,
        stderr,
    })
}

/// Opens a new file at `path` only for writing, as a shell redirection opens one.
fn create(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// A new directory for one run's stream files in the temporary directory, named
/// and protected as the scratch directory is, and removed when dropped.
struct StreamDir {
    path: PathBuf,
}

impl StreamDir {
    fn create() -> io::Result<StreamDir> {
        Ok(StreamDir {
            path: platform::create_scratch_dir(&env::temp_dir())?,
        })
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for StreamDir {
    fn drop(&mut self) {
        platform::remove_scratch_dir(&self.path);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> SshOutput {
        SystemSsh::with_program("sh")
            .run(&["-c".to_string(), script.to_string()], b"", true)
            .unwrap()
    }

    #[test]
    fn r01_captured_stderr_is_not_a_pipe() {
        let output = sh("if [ -p /dev/stderr ]; then echo pipe; else echo other; fi");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "other\n");
    }

    #[test]
    fn r02_captured_stderr_holds_what_the_program_wrote() {
        let output = sh("echo out; echo err >&2; exit 3");
        assert_eq!(output.status, Some(3));
        assert_eq!(output.stdout, b"out\n");
        assert_eq!(output.stderr, b"err\n");
    }

    #[test]
    fn r03_uncaptured_stderr_is_not_a_pipe() {
        let output = SystemSsh::with_program("sh")
            .run(
                &[
                    "-c".to_string(),
                    "if [ -p /dev/stderr ]; then echo pipe; else echo other; fi".to_string(),
                ],
                b"",
                false,
            )
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "other\n");
    }

    #[test]
    fn r04_stdin_and_stdout_are_not_pipes() {
        let output =
            sh("[ -p /dev/stdin ] && echo stdin; [ -p /dev/stdout ] && echo stdout >&2; :");
        assert_eq!(output.stdout, b"");
        assert_eq!(output.stderr, b"");
    }

    #[test]
    fn r05_stdin_carries_the_given_bytes() {
        let output = SystemSsh::with_program("sh")
            .run(
                &["-c".to_string(), "cat".to_string()],
                b"key 1\nkey 2",
                true,
            )
            .unwrap();
        assert_eq!(output.stdout, b"key 1\nkey 2");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn r06_stream_files_are_in_a_directory_only_the_owner_can_enter() {
        let output =
            sh(r#"d=$(dirname "$(readlink /proc/self/fd/0)"); stat -c %a "$d"; basename "$d""#);
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("700"), "{text}");
        let name = lines.next().unwrap();
        assert!(name.starts_with("ssh-copy-id."), "{text}");
        assert!(
            !std::path::Path::new(&std::env::temp_dir())
                .join(name)
                .exists()
        );
    }
}
