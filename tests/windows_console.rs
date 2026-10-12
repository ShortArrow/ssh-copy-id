//! A console without a window, as `CREATE_NO_WINDOW` gives a process started by
//! an IDE task, a GUI launcher, or a CI runner, is one nobody can type into, so
//! the CLI treats it as no console and runs `ssh` with `BatchMode=yes`.
#![cfg(windows)]

use std::fs;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;
const BATCH_MODE_NOTICE: &str = "there is no console and SSH_ASKPASS is not set";

/// Runs the CLI with `flags` against a closed local port, with a fresh home
/// holding `~/.ssh` and a selected key, and returns its stderr.
fn stderr_when_started_with(flags: u32, name: &str) -> String {
    let home = std::env::temp_dir().join(format!("windows-console-{name}-{}", std::process::id()));
    fs::create_dir_all(home.join(".ssh")).unwrap();
    fs::write(home.join("key"), "").unwrap();
    fs::write(
        home.join("key.pub"),
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== me@here\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ssh-copy-id"))
        .args(["-i"])
        .arg(home.join("key.pub"))
        .args([
            "-F",
            "none",
            "-p",
            "1",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "IdentityAgent=none",
            "u@127.0.0.1",
        ])
        .env("USERPROFILE", &home)
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .creation_flags(flags)
        .output()
        .unwrap();
    fs::remove_dir_all(&home).unwrap();
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn w01_a_console_without_a_window_runs_ssh_in_batch_mode() {
    let stderr = stderr_when_started_with(CREATE_NO_WINDOW, "hidden");
    assert!(stderr.contains(BATCH_MODE_NOTICE), "{stderr}");
}

#[test]
fn w02_no_console_runs_ssh_in_batch_mode() {
    let stderr = stderr_when_started_with(DETACHED_PROCESS, "detached");
    assert!(stderr.contains(BATCH_MODE_NOTICE), "{stderr}");
}
