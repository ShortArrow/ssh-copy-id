//! Stage 1 behavior against the L02 Linux fixture: requirements 1, 2, 3, 7, and 8,
//! and design D-01, D-15, D-16, D-17, and D-18. Every run of the CLI must leave
//! no `ssh-copy-id.*` scratch directory in the local `~/.ssh`.
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
        Fixture::start_with(&[])
    }

    /// Starts a container with `run_args` added to `docker run`, such as a mount.
    fn start_with(run_args: &[&str]) -> Fixture {
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
    /// test when the run takes longer than `COPY_ID_TIMEOUT` or leaves a scratch
    /// directory behind in the local `~/.ssh`.
    fn copy_id_answering(&self, password: &str, key: &Path, user: &str, extra: &[&str]) -> Output {
        let before = scratch_entries();
        let output = self.run_copy_id(password, key, user, extra);
        let left: Vec<String> = scratch_entries()
            .into_iter()
            .filter(|name| !before.contains(name))
            .collect();
        assert!(
            left.is_empty(),
            "scratch directories left in {}: {left:?}",
            local_ssh_directory().display()
        );
        output
    }

    fn run_copy_id(&self, password: &str, key: &Path, user: &str, extra: &[&str]) -> Output {
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

    fn exec_as_root(&self, script: &str) -> Output {
        command("docker", &["exec", &self.container, "sh", "-c", script])
    }

    /// Starts a container whose `/home/pwuser/.ssh` is a 16 KiB tmpfs.
    fn start_with_small_ssh_directory() -> Fixture {
        let fixture = Fixture::start_with(&["--tmpfs", "/home/pwuser/.ssh:size=16k,mode=0700"]);
        let owned = fixture.exec_as_root("chown pwuser:pwuser /home/pwuser/.ssh");
        assert!(owned.status.success(), "{}", text(&owned.stderr));
        fixture
    }

    /// Writes pwuser's `authorized_keys` as one line ending `room` bytes before a
    /// page boundary, takes every other page of the tmpfs, and returns the file.
    fn fill_ssh_directory_leaving(&self, room: usize) -> String {
        let fill = self.exec(
            "pwuser",
            &format!(
                r#"umask 077 && page=$(getconf PAGESIZE) \
            && {{ head -c $((page - {room} - 1)) /dev/zero | tr '\0' x; printf '\n'; }} > .ssh/authorized_keys \
            && {{ dd if=/dev/zero of=.ssh/filler bs=1024 2>/dev/null; true; }} \
            && [ "$(df -P .ssh | awk 'NR == 2 {{ print $4 }}')" = 0 ]"#
            ),
        );
        assert!(fill.status.success(), "{}", text(&fill.stderr));
        self.read_as_root("/home/pwuser/.ssh/authorized_keys")
    }

    /// The octal permission bits of `path`, relative to `user`'s home directory.
    fn mode(&self, user: &str, path: &str) -> String {
        text(&self.exec(user, &format!("stat -c %a {path}")).stdout)
            .trim()
            .to_string()
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

/// The local `~/.ssh`, from the variable the CLI reads its home directory from.
fn local_ssh_directory() -> PathBuf {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(std::env::var_os(variable).unwrap()).join(".ssh")
}

/// The names in the local `~/.ssh` that start with `ssh-copy-id.`.
fn scratch_entries() -> Vec<String> {
    fs::read_dir(local_ssh_directory())
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("ssh-copy-id."))
                .collect()
        })
        .unwrap_or_default()
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
    assert_eq!(fixture.mode("pwuser", ".ssh"), "700");
    assert_eq!(fixture.mode("pwuser", ".ssh/authorized_keys"), "600");
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
fn i6_requirement_7_comments_with_spaces_quotes_japanese_and_format_text_are_data() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", r#"名前 it's "quoted" $HOME %s \t"#);
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
    assert!(fixture.logs_in_with(&key, "pwuser"));
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
fn i14_d01_a_banner_forging_publickey_authentication_is_not_taken_as_installed() {
    let fixture = Fixture::start();
    let banner = fixture.read_as_root("/etc/ssh/emptyuser-banner");
    assert!(
        banner.contains(r#"Authenticated to 127.0.0.1 ([127.0.0.1]:22) using "publickey"."#),
        "{banner}"
    );
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

#[test]
#[ignore = "needs the L02 fixture image"]
fn i15_d18_lines_are_trimmed_trailing_blanks_dropped_and_only_the_key_is_counted() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let line = public_line(&key);
    fs::write(
        key.with_extension("pub"),
        format!("# laptop\n\n  # indented\n\t\n{line}  \n\n\t\n"),
    )
    .unwrap();
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(text(&run.stdout).contains("Number of key(s) added: 1\n"));
    assert_eq!(
        fixture.authorized_keys("pwuser"),
        format!("# laptop\n\n# indented\n\n{line}\n")
    );
}

#[test]
#[ignore = "needs the L02 fixture image"]
fn i16_d16_a_fifo_target_is_not_written_and_does_not_hang() {
    let fixture = Fixture::start();
    let setup = fixture.exec(
        "pwuser",
        "umask 077 && mkdir -p .ssh && mkfifo .ssh/authorized_keys",
    );
    assert!(setup.status.success());
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert!(
        fixture
            .exec("pwuser", "test -p .ssh/authorized_keys")
            .status
            .success()
    );
}

/// The existing file ends 6 bytes before a page boundary and every other page of
/// the size-limited tmpfs is taken, so the key line is written in part and then
/// fails with ENOSPC.
#[test]
#[ignore = "needs the L02 fixture image"]
fn i17_d17_a_write_cut_short_by_a_full_filesystem_is_rolled_back() {
    let fixture = Fixture::start_with_small_ssh_directory();
    let before = fixture.fill_ssh_directory_leaving(6);
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, "pwuser", &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the key was not written to"), "{stderr}");
    assert_eq!(
        fixture.read_as_root("/home/pwuser/.ssh/authorized_keys"),
        before
    );
}

/// The comment line fits in the 32 bytes left before the page boundary and the
/// key line does not, so the comment is rolled back with the key that failed.
#[test]
#[ignore = "needs the L02 fixture image"]
fn i18_d17_a_comment_line_is_rolled_back_with_the_key_that_failed() {
    let fixture = Fixture::start_with_small_ssh_directory();
    let before = fixture.fill_ssh_directory_leaving(32);
    let key = keygen(&fixture.work, "new", "new@test");
    let line = public_line(&key);
    assert!(line.len() + 1 > 32 - "# note\n".len());
    fs::write(key.with_extension("pub"), format!("# note\n{line}\n")).unwrap();
    let run = fixture.copy_id(&key, "pwuser", &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the key was not written to"), "{stderr}");
    assert_eq!(
        fixture.read_as_root("/home/pwuser/.ssh/authorized_keys"),
        before
    );
}

/// The key's options make every session it opens exit 1, so the check that finds
/// it installed sees ssh fail after authenticating with it.
#[test]
#[ignore = "needs the L02 fixture image"]
fn i19_d01_a_key_whose_command_exits_nonzero_is_found_installed() {
    let fixture = Fixture::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let line = format!("command=\"exit 1\" {}", public_line(&key));
    fs::write(key.with_extension("pub"), format!("{line}\n")).unwrap();
    let first = fixture.copy_id(&key, "pwuser", &[]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));
    assert!(text(&first.stdout).contains("Number of key(s) added: 1\n"));
    let again = fixture.copy_id(&key, "pwuser", &[]);
    let stderr = text(&again.stderr);
    assert_eq!(again.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("All keys were skipped"), "{stderr}");
    assert_eq!(fixture.authorized_keys("pwuser"), format!("{line}\n"));
}
