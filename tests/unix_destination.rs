//! Stage 1 behavior against the L02 Linux fixture: requirements 1, 2, 3, 7, and 8,
//! and design D-15.
//!
//! Each test starts its own container from the `ssh-copy-id-l02:local` image on a
//! free loopback port. Build the image first and run these tests explicitly:
//!
//! ```text
//! docker build -t ssh-copy-id-l02:local tests/environments/linux
//! cargo test --test unix_destination -- --ignored --test-threads=1
//! ```

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const IMAGE: &str = "ssh-copy-id-l02:local";
const COPY_ID_TIMEOUT: Duration = Duration::from_secs(60);
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    container: String,
    port: String,
    work: PathBuf,
    password: String,
    control: PathBuf,
}

impl Fixture {
    fn start() -> Fixture {
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
        let run = command(
            "docker",
            &[
                "run",
                "-d",
                "-p",
                "127.0.0.1::22",
                "-e",
                &format!("PWUSER_PASSWORD={password}"),
                "-e",
                &format!("CONTROL_PUBLIC_KEY={}", public.trim()),
                IMAGE,
            ],
        );
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

    fn known_hosts(&self) -> PathBuf {
        self.work.join("known_hosts")
    }

    fn base_ssh_args(&self) -> Vec<String> {
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

    fn askpass(&self, password: &str) -> PathBuf {
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

    fn copy_id(&self, key: &Path, user: &str, extra: &[&str]) -> Output {
        self.copy_id_answering(&self.password, key, user, extra)
    }

    /// Runs the CLI with an askpass program that answers `password`, and fails the
    /// test when the run takes longer than `COPY_ID_TIMEOUT`.
    fn copy_id_answering(&self, password: &str, key: &Path, user: &str, extra: &[&str]) -> Output {
        let mut args: Vec<String> = vec![
            "-i".into(),
            key.display().to_string(),
            "-p".into(),
            self.port.clone(),
            "-F".into(),
            "none".into(),
            "-o".into(),
            format!("UserKnownHostsFile={}", self.known_hosts().display()),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-o".into(),
            "ConnectTimeout=5".into(),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        args.push(format!("{user}@127.0.0.1"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_ssh-copy-id"))
            .args(&args)
            .env("SSH_ASKPASS", self.askpass(password))
            .env("SSH_ASKPASS_REQUIRE", "force")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = drain(child.stdout.take().unwrap());
        let stderr = drain(child.stderr.take().unwrap());
        let deadline = Instant::now() + COPY_ID_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                panic!("ssh-copy-id did not finish within {COPY_ID_TIMEOUT:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        Output {
            status,
            stdout: stdout.join().unwrap(),
            stderr: stderr.join().unwrap(),
        }
    }

    fn exec(&self, user: &str, script: &str) -> Output {
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

    fn authorized_keys(&self, user: &str) -> String {
        text(
            &self
                .exec(user, "cat .ssh/authorized_keys 2>/dev/null")
                .stdout,
        )
    }

    fn read_as_root(&self, path: &str) -> String {
        text(&command("docker", &["exec", &self.container, "cat", path]).stdout)
    }

    fn logs_in_with(&self, key: &Path, user: &str) -> bool {
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

fn command(program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn keygen(dir: &Path, name: &str, comment: &str) -> PathBuf {
    let path = dir.join(name);
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(made.status.success(), "ssh-keygen: {}", text(&made.stderr));
    path
}

fn public_line(key: &Path) -> String {
    fs::read_to_string(key.with_extension("pub"))
        .unwrap()
        .trim_end()
        .to_string()
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i1_requirement_2_installs_into_a_missing_file_and_the_key_logs_in() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(text(&run.stdout).contains("Number of key(s) added: 1"));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
    assert!(fixture.logs_in_with(&key, "pwuser"));
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i2_requirement_1_a_second_run_adds_nothing() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    assert_eq!(fixture.copy_id(&key, "pwuser", &[]).status.code(), Some(0));
    let again = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again.stderr));
    assert!(text(&again.stderr).contains("All keys were skipped"));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i3_requirement_1_success_with_another_key_is_not_taken_as_installed() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let control = format!("IdentityFile={}", fixture.control.display());
    let run = fixture.copy_id(&key, "keyuser", &["-o", &control]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(
        fixture
            .authorized_keys("keyuser")
            .contains(&public_line(&key))
    );
    assert!(fixture.logs_in_with(&key, "keyuser"));
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i4_requirement_2_appends_after_a_missing_final_newline() {
    let fixture = Fixture::start();
    let setup = fixture.exec(
        "pwuser",
        "umask 077 && mkdir -p .ssh && printf 'existing' > .ssh/authorized_keys",
    );
    assert!(setup.status.success());
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("existing\n{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i5_requirement_3_an_unwritable_file_is_kept_and_reported() {
    let fixture = Fixture::start();
    let setup = fixture.exec(
        "pwuser",
        "umask 077 && mkdir -p .ssh && printf 'existing\\n' > .ssh/authorized_keys && chmod 400 .ssh/authorized_keys",
    );
    assert!(setup.status.success());
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert_eq!(fixture.authorized_keys("pwuser"), "existing\n");
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i6_requirement_7_comments_with_spaces_quotes_and_japanese_are_data() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "名前 it's \"quoted\" $HOME");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i7_requirement_8_a_host_key_mismatch_writes_nothing() {
    let fixture = Fixture::start();
    let other = keygen(&fixture.work, "fake_host", "fake");
    let host = format!("[127.0.0.1]:{}", fixture.port);
    fs::write(
        fixture.known_hosts(),
        format!("{host} {}\n", public_line(&other)),
    )
    .unwrap();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Host key verification failed"), "{stderr}");
    assert!(
        !stderr.contains("remain to be installed"),
        "the run must stop at the check, before the installation step: {stderr}"
    );
    assert_eq!(fixture.authorized_keys("pwuser"), "");
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i8_a_tcsh_login_shell_runs_the_installation_command() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "cshuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.authorized_keys("cshuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i9_requirement_2_an_empty_file_gets_no_leading_newline() {
    let fixture = Fixture::start();
    let setup = fixture.exec(
        "pwuser",
        "umask 077 && mkdir -p .ssh && : > .ssh/authorized_keys",
    );
    assert!(setup.status.success());
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i10_requirement_3_an_unreadable_file_is_not_written() {
    let fixture = Fixture::start();
    let setup = fixture.exec(
        "pwuser",
        "umask 077 && mkdir -p .ssh && printf 'existing' > .ssh/authorized_keys && chmod 200 .ssh/authorized_keys",
    );
    assert!(setup.status.success());
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert_eq!(
        fixture.read_as_root("/home/pwuser/.ssh/authorized_keys"),
        "existing"
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i11_requirement_8_a_failed_password_login_writes_nothing() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id_answering("wrong-password", &key, "pwuser", &[]);
    let stderr = text(&run.stderr);
    let stdout = text(&run.stdout);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(!stderr.contains("Number of key(s) added"), "{stderr}");
    assert!(!stdout.contains("Number of key(s) added"), "{stdout}");
    assert!(
        fixture
            .exec("pwuser", "test ! -e .ssh/authorized_keys")
            .status
            .success()
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i12_requirement_7_key_paths_with_spaces_quotes_and_japanese_are_used_as_given() {
    let fixture = Fixture::start();
    let dir = fixture.work.join("dir 名前 'q'");
    fs::create_dir_all(&dir).unwrap();
    let key = keygen(&dir, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("the key authenticates"), "{stderr}");
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i13_d15_a_forced_tty_does_not_hang_the_installation() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &["-o", "RequestTTY=force"]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("{}\n", public_line(&key))
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i14_requirement_1_none_authentication_is_not_taken_as_installed() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "emptyuser", &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains("All keys were skipped"), "{stderr}");
    assert_eq!(
        fixture.authorized_keys("emptyuser"),
        format!("{}\n", public_line(&key))
    );
}
