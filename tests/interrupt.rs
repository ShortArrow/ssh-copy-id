//! A console interrupt while `ssh` runs: the CLI outlives it, waits for `ssh`,
//! exits with status 1, and leaves no `ssh-copy-id.*` scratch directory in `~/.ssh`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== me@here";
const TIMEOUT: Duration = Duration::from_secs(20);

struct Work(PathBuf);

impl Work {
    fn new() -> Work {
        let path =
            std::env::temp_dir().join(format!("ssh-copy-id-interrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("home/.ssh")).unwrap();
        fs::create_dir_all(path.join("bin")).unwrap();
        Work(path)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for(condition: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "{what} within {TIMEOUT:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Sends `signal` to the process group with the `kill` program, not a shell
/// builtin: dash's builtin rejects `--` and sends nothing.
fn signal_group(pgid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([signal, "--", &format!("-{pgid}")])
        .status()
        .expect("kill must be on PATH for this test");
    assert!(status.success(), "kill {signal} -- -{pgid} failed");
}

fn scratch_entries(ssh_dir: &Path) -> Vec<String> {
    fs::read_dir(ssh_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ssh-copy-id."))
        .collect()
}

#[test]
fn i01_an_interrupt_while_ssh_runs_exits_1_and_removes_the_scratch_directory() {
    let work = Work::new();
    let home = work.0.join("home");
    let started = work.0.join("ssh-started");
    let release = work.0.join("ssh-release");
    let fake_ssh = work.0.join("bin/ssh");
    fs::write(
        &fake_ssh,
        format!(
            "#!/bin/sh\n: > '{}'\nwhile [ ! -e '{}' ]; do sleep 1; done\nexit 255\n",
            started.display(),
            release.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_ssh, fs::Permissions::from_mode(0o755)).unwrap();
    let key = work.0.join("id");
    fs::write(&key, "private").unwrap();
    fs::write(key.with_extension("pub"), format!("{KEY}\n")).unwrap();
    let path = format!(
        "{}:{}",
        work.0.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut cli = Command::new(env!("CARGO_BIN_EXE_ssh-copy-id"))
        .args(["-i", &key.display().to_string(), "u@h"])
        .env("HOME", &home)
        .env("PATH", path)
        .env_remove("SSH_ASKPASS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let pgid = cli.id();
    wait_for(|| started.exists(), "the fake ssh starts");
    signal_group(pgid, "-INT");
    fs::write(&release, "").unwrap();

    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = cli.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            signal_group(pgid, "-KILL");
            let _ = cli.wait();
            panic!("the CLI did not exit within {TIMEOUT:?} of the interrupt");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = cli.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(status.code(), Some(1), "{status:?}\n{stderr}");
    assert!(
        stderr.contains("ssh-copy-id: ERROR: interrupted"),
        "{stderr}"
    );
    assert_eq!(scratch_entries(&home.join(".ssh")), Vec::<String>::new());
}
