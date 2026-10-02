//! The real `Ssh` backend: runs the system `ssh` client as a child process.

use crate::app::{Ssh, SshOutput};
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;

/// Runs `ssh` found on `PATH`, or the program given to `SystemSsh::with_program`.
pub struct SystemSsh {
    program: OsString,
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
        }
    }
}

impl Default for SystemSsh {
    fn default() -> SystemSsh {
        SystemSsh::new()
    }
}

impl Ssh for SystemSsh {
    /// Starts `ssh` with stdin and stdout piped. stdin is written from another
    /// thread so that a large input cannot block while `ssh` waits to write output.
    fn run(
        &mut self,
        args: &[String],
        stdin: &[u8],
        capture_stderr: bool,
    ) -> io::Result<SshOutput> {
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if capture_stderr {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .spawn()?;
        let mut pipe = child.stdin.take().expect("stdin is piped");
        let data = stdin.to_vec();
        let writer = thread::spawn(move || {
            let result = pipe.write_all(&data);
            drop(pipe);
            result
        });
        let output = child.wait_with_output()?;
        match writer.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) if e.kind() == io::ErrorKind::BrokenPipe => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::other("stdin writer panicked")),
        }
        Ok(SshOutput {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}
