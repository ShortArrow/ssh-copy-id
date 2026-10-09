//! Behavior against the L03 OpenWrt fixture, BusyBox ash and Dropbear as on a
//! router: the default target of root (design: special destinations), D-22, and
//! D-17 on a destination whose BusyBox has no `od`.
//!
//! Each test starts its own container from the `ssh-copy-id-l03:local` image on a
//! free loopback port and installs keys for root, who authenticates with a
//! per-run password. Build the image first and run these tests explicitly:
//!
//! ```text
//! docker build -t ssh-copy-id-l03:local tests/environments/openwrt
//! cargo test --test openwrt_destination -- --ignored --test-threads=1
//! ```

mod common;

use common::{
    base_ssh_args, command, keygen, public_line, run_container, text, wait_with_deadline,
    write_askpass,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const IMAGE: &str = "ssh-copy-id-l03:local";
const DROPBEAR_KEYS: &str = "/etc/dropbear/authorized_keys";
const COPY_ID_TIMEOUT: Duration = Duration::from_secs(60);
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A container from the L03 image with its own work directory, root password,
/// and control key, which the entrypoint writes as the only line of
/// `/etc/dropbear/authorized_keys`.
struct OpenWrt {
    container: String,
    port: String,
    work: PathBuf,
    password: String,
    control: PathBuf,
}

impl OpenWrt {
    fn start() -> OpenWrt {
        OpenWrt::start_with(&[])
    }

    /// Starts a container with `run_args` added to `docker run`, such as a mount.
    fn start_with(run_args: &[&str]) -> OpenWrt {
        let work = std::env::temp_dir().join(format!(
            "ssh-copy-id-l03-test-{}-{}",
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
        let (container, port) = run_container(
            IMAGE,
            &[
                format!("ROOT_PASSWORD={password}"),
                format!("CONTROL_PUBLIC_KEY={}", public_line(&control)),
            ],
            run_args,
        );
        let fixture = OpenWrt {
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
        while !self.logs_in_with(&self.control) {
            assert!(Instant::now() < deadline, "fixture not ready");
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn ssh_args(&self) -> Vec<String> {
        base_ssh_args(&self.port, &self.work.join("known_hosts"))
    }

    fn logs_in_with(&self, key: &Path) -> bool {
        Command::new("ssh")
            .args(self.ssh_args())
            .args(["-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes", "-i"])
            .arg(key)
            .args(["root@127.0.0.1", "true"])
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    }

    /// Runs the CLI for root with `-i key`, `extra`, and an askpass program that
    /// answers root's password, and fails the test when it runs longer than
    /// `COPY_ID_TIMEOUT`.
    fn copy_id(&self, key: &Path, extra: &[&str]) -> Output {
        let mut args: Vec<String> = vec!["-i".into(), key.display().to_string()];
        args.extend(self.ssh_args());
        args.extend(extra.iter().map(|s| s.to_string()));
        args.push("root@127.0.0.1".into());
        let child = Command::new(env!("CARGO_BIN_EXE_ssh-copy-id"))
            .args(&args)
            .env("SSH_ASKPASS", write_askpass(&self.work, &self.password))
            .env("SSH_ASKPASS_REQUIRE", "force")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_with_deadline(child, COPY_ID_TIMEOUT)
    }

    fn exec(&self, script: &str) -> Output {
        command("docker", &["exec", &self.container, "sh", "-c", script])
    }

    fn read(&self, path: &str) -> String {
        text(&command("docker", &["exec", &self.container, "cat", path]).stdout)
    }

    /// Starts a container whose `/etc/dropbear` is a 16 KiB tmpfs.
    fn start_with_small_dropbear_directory() -> OpenWrt {
        OpenWrt::start_with(&["--tmpfs", "/etc/dropbear:size=16k,mode=0755"])
    }

    /// Writes root's key file as one line ending `room` bytes before the 4 KiB
    /// page boundary of x86-64, takes every other page of the tmpfs, and returns
    /// the file.
    fn fill_dropbear_directory_leaving(&self, room: usize) -> String {
        let fill = self.exec(&format!(
            r#"{{ head -c $((4096 - {room} - 1)) /dev/zero | tr '\0' x; printf '\n'; }} > {DROPBEAR_KEYS} \
            && {{ dd if=/dev/zero of=/etc/dropbear/filler bs=1024 2>/dev/null; true; }} \
            && [ "$(df -P /etc/dropbear | awk 'NR == 2 {{ print $4 }}')" = 0 ]"#
        ));
        assert!(fill.status.success(), "{}", text(&fill.stderr));
        self.read(DROPBEAR_KEYS)
    }

    /// Replaces `tail` with a wrapper whose first run appends `line` and a
    /// newline to root's key file before it runs BusyBox's `tail`, as another
    /// writer appending while the installation runs.
    fn append_before_the_first_tail(&self, line: &str) {
        let wrap = self.exec(&format!(
            r#"mkdir -p /tmp/real && ln -s /bin/busybox /tmp/real/tail && rm /usr/bin/tail \
            && printf '%s\n' '#!/bin/sh' \
                '[ -e /tmp/real/done ] || {{ echo {line} >> {DROPBEAR_KEYS}; : > /tmp/real/done; }}' \
                'exec /tmp/real/tail "$@"' > /usr/bin/tail \
            && chmod 755 /usr/bin/tail"#
        ));
        assert!(wrap.status.success(), "{}", text(&wrap.stderr));
    }
}

impl Drop for OpenWrt {
    fn drop(&mut self) {
        let _ = command("docker", &["rm", "-f", &self.container]);
        let _ = fs::remove_dir_all(&self.work);
    }
}

#[test]
#[ignore = "needs the L03 fixture image"]
fn o1_root_without_target_gets_the_key_in_the_dropbear_file_and_it_logs_in() {
    let fixture = OpenWrt::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(0), "{stderr}");
    assert!(text(&run.stdout).contains("Number of key(s) added: 1"));
    let keys = fixture.read(DROPBEAR_KEYS);
    assert!(keys.lines().any(|line| line == public_line(&key)), "{keys}");
    assert!(fixture.exec("[ ! -e /root/.ssh ]").status.success());
    assert!(fixture.logs_in_with(&key));
}

/// The control key's line ends with a newline, so the key line follows it
/// directly, as on every other destination.
#[test]
#[ignore = "needs the L03 fixture image"]
fn o2_the_key_line_follows_a_final_newline_directly() {
    let fixture = OpenWrt::start();
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, &[]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.read(DROPBEAR_KEYS),
        format!("{}\n{}\n", public_line(&fixture.control), public_line(&key))
    );
}

#[test]
#[ignore = "needs the L03 fixture image"]
fn o3_d22_an_explicit_target_is_written_instead_of_the_dropbear_file() {
    let fixture = OpenWrt::start();
    let before = fixture.read(DROPBEAR_KEYS);
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, &["-t", ".ssh/authorized_keys"]);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        fixture.read("/root/.ssh/authorized_keys"),
        format!("{}\n", public_line(&key))
    );
    assert_eq!(fixture.read(DROPBEAR_KEYS), before);
}

#[test]
#[ignore = "needs the L03 fixture image"]
fn o4_d17_a_write_cut_short_by_a_full_filesystem_is_rolled_back() {
    let fixture = OpenWrt::start_with_small_dropbear_directory();
    let before = fixture.fill_dropbear_directory_leaving(6);
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the key was not written to"), "{stderr}");
    assert_eq!(fixture.read(DROPBEAR_KEYS), before);
}

/// Another writer appends a line after the installation took the file's size
/// and before its key line fails to fit, so the bytes after that size are not
/// the ones the installation wrote, and the file is not truncated.
#[test]
#[ignore = "needs the L03 fixture image"]
fn o5_d17_a_line_another_writer_appended_is_kept() {
    let fixture = OpenWrt::start_with_small_dropbear_directory();
    let before = fixture.fill_dropbear_directory_leaving(32);
    fixture.append_before_the_first_tail("other");
    let key = keygen(&fixture.work, "new", "new@test");
    let run = fixture.copy_id(&key, &[]);
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("the partial line could not be removed"),
        "{stderr}"
    );
    let after = fixture.read(DROPBEAR_KEYS);
    assert!(after.starts_with(&format!("{before}other\n")), "{after:?}");
}
