//! The L02 Linux fixture shared by the tests that run against it: a container
//! from the `ssh-copy-id-l02:local` image on a free loopback port, with its own
//! work directory, pwuser password, and keyuser control key.
#![allow(dead_code)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const IMAGE: &str = "ssh-copy-id-l02:local";
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Fixture {
    pub container: String,
    pub port: String,
    pub work: PathBuf,
    pub password: String,
    pub control: PathBuf,
}

impl Fixture {
    pub fn start() -> Fixture {
        Fixture::start_with(&[])
    }

    /// Starts a container with `run_args` added to `docker run`, such as a mount.
    pub fn start_with(run_args: &[&str]) -> Fixture {
        let work = std::env::temp_dir().join(format!(
            "ssh-copy-id-l02-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&work).unwrap();
        let control = keygen(&work, "control", "control");
        let password = format!(
            "pw-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let public = fs::read_to_string(control.with_extension("pub")).unwrap();
        let password_env = format!("PWUSER_PASSWORD={password}");
        let control_env = format!("CONTROL_PUBLIC_KEY={}", public.trim());
        let mut args = vec![
            "run",
            "-d",
            "-p",
            "127.0.0.1::22",
            "-e",
            &password_env,
            "-e",
            &control_env,
        ];
        args.extend_from_slice(run_args);
        args.push(IMAGE);
        let run = command("docker", &args);
        assert!(run.status.success(), "docker run: {}", text(&run.stderr));
        let container = text(&run.stdout).trim().to_string();
        let mapped = command("docker", &["port", &container, "22/tcp"]);
        let port = text(&mapped.stdout)
            .lines()
            .next()
            .and_then(|line| line.rsplit(':').next())
            .unwrap()
            .trim()
            .to_string();
        let fixture = Fixture {
            container,
            port,
            work,
            password,
            control,
        };
        fixture.wait_until_ready();
        fixture
    }

    fn wait_until_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let probe = Command::new("ssh")
                .args(self.base_ssh_args())
                .args(["-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes", "-i"])
                .arg(&self.control)
                .args(["keyuser@127.0.0.1", "true"])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            if probe.status.success() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "fixture not ready: {}",
                text(&probe.stderr)
            );
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn known_hosts(&self) -> PathBuf {
        self.work.join("known_hosts")
    }

    pub fn base_ssh_args(&self) -> Vec<String> {
        vec![
            "-F".into(),
            "none".into(),
            "-p".into(),
            self.port.clone(),
            "-o".into(),
            format!("UserKnownHostsFile={}", self.known_hosts().display()),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-o".into(),
            "ConnectTimeout=5".into(),
        ]
    }

    pub fn askpass(&self, password: &str) -> PathBuf {
        if cfg!(windows) {
            let path = self.work.join("askpass.cmd");
            fs::write(&path, format!("@echo {password}\r\n")).unwrap();
            path
        } else {
            let path = self.work.join("askpass");
            fs::write(&path, format!("#!/bin/sh\necho '{password}'\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            path
        }
    }

    pub fn exec(&self, user: &str, script: &str) -> Output {
        command(
            "docker",
            &[
                "exec",
                "-u",
                user,
                "-w",
                &format!("/home/{user}"),
                &self.container,
                "sh",
                "-c",
                script,
            ],
        )
    }

    pub fn authorized_keys(&self, user: &str) -> String {
        text(
            &self
                .exec(user, "cat .ssh/authorized_keys 2>/dev/null")
                .stdout,
        )
    }

    pub fn read_as_root(&self, path: &str) -> String {
        text(&command("docker", &["exec", &self.container, "cat", path]).stdout)
    }

    pub fn exec_as_root(&self, script: &str) -> Output {
        command("docker", &["exec", &self.container, "sh", "-c", script])
    }

    /// The octal permission bits of `path`, relative to `user`'s home directory.
    pub fn mode(&self, user: &str, path: &str) -> String {
        text(&self.exec(user, &format!("stat -c %a {path}")).stdout)
            .trim()
            .to_string()
    }

    pub fn logs_in_with(&self, key: &Path, user: &str) -> bool {
        Command::new("ssh")
            .args(self.base_ssh_args())
            .args(["-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes", "-i"])
            .arg(key)
            .args([&format!("{user}@127.0.0.1"), "true"])
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = command("docker", &["rm", "-f", &self.container]);
        let _ = fs::remove_dir_all(&self.work);
    }
}

pub fn command(program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// Collects the output of a child whose stdout and stderr are piped, and panics
/// when it runs longer than `timeout`.
pub fn wait_with_deadline(mut child: Child, timeout: Duration) -> Output {
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("ssh-copy-id did not finish within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn keygen(dir: &Path, name: &str, comment: &str) -> PathBuf {
    let path = dir.join(name);
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(made.status.success(), "ssh-keygen: {}", text(&made.stderr));
    path
}

pub fn public_line(key: &Path) -> String {
    fs::read_to_string(key.with_extension("pub"))
        .unwrap()
        .trim_end()
        .to_string()
}
