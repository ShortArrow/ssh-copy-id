//! Behavior against a dockur Windows guest: requirements 1, 2, 3, 7, and 8 for a
//! standard user, and the shared administrator file. Every run of the CLI
//! uses a temporary `USERPROFILE`, the Windows OpenSSH client first on `PATH`,
//! its own known-hosts file, no agent, and an `IdentityFile`, so that `ssh`
//! reads none of the local account's keys.
//!
//! The guest is one of the fixtures in `tests/environments/windows`, with
//! `fixtureuser` signed in once and an administrator control key installed.
//! Name it with environment variables and run the tests serially:
//!
//! ```text
//! $env:SSH_COPY_ID_WINDOWS_PORT = '22223'
//! $env:SSH_COPY_ID_WINDOWS_KNOWN_HOSTS = 'work/windows-fixture-ps/known_hosts'
//! $env:SSH_COPY_ID_WINDOWS_CONTROL_KEY = 'work/windows-fixture-ps/smoke_key'
//! cargo test --test windows_destination -- --ignored --test-threads=1
//! ```
#![cfg(windows)]

mod common;

use common::{public_entry, text, wait_with_deadline};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const COPY_ID_TIMEOUT: Duration = Duration::from_secs(120);
const PASSWORD: &str = "Fixture-Only_47!";
const USER_KEYS: &str = r"C:\Users\fixtureuser\.ssh\authorized_keys";
const ADMIN_KEYS: &str = r"C:\ProgramData\ssh\administrators_authorized_keys";

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn openssh(program: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join(r"System32\OpenSSH")
        .join(program)
}

/// The guest named by the environment, with a fresh local work directory and
/// `fixtureuser`'s `.ssh` removed, so that each test starts from a profile
/// without key files.
struct Guest {
    port: String,
    known_hosts: PathBuf,
    control_key: PathBuf,
    work: PathBuf,
}

impl Guest {
    fn start() -> Guest {
        let variable = |name: &str| {
            std::env::var(name).unwrap_or_else(|_| panic!("{name} names the Windows fixture"))
        };
        let work = std::env::temp_dir().join(format!(
            "ssh-copy-id-windows-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(work.join("home").join(".ssh")).unwrap();
        let guest = Guest {
            port: variable("SSH_COPY_ID_WINDOWS_PORT"),
            known_hosts: std::path::absolute(variable("SSH_COPY_ID_WINDOWS_KNOWN_HOSTS")).unwrap(),
            control_key: std::path::absolute(variable("SSH_COPY_ID_WINDOWS_CONTROL_KEY")).unwrap(),
            work,
        };
        guest.admin(
            "if(Test-Path C:\\Users\\fixtureuser\\.ssh){icacls C:\\Users\\fixtureuser\\.ssh /reset /t /q|Out-Null;\
             Remove-Item -Recurse -Force C:\\Users\\fixtureuser\\.ssh};'ok'",
        );
        guest
    }

    /// Runs a Windows PowerShell script as `fixtureadmin` through the control
    /// key, whatever the default shell, and returns its stdout.
    fn admin(&self, script: &str) -> String {
        let mut ssh = Command::new(openssh("ssh.exe"));
        ssh.args(["-F", "none", "-i"])
            .arg(&self.control_key)
            .args(["-o", "BatchMode=yes"]);
        self.admin_through(ssh, script)
    }

    /// Runs a script as [`Guest::admin`] does, logging in with the password,
    /// so that it works whatever the shared administrator file holds.
    fn admin_by_password(&self, script: &str) -> String {
        let mut ssh = Command::new(openssh("ssh.exe"));
        ssh.args(["-F", "none"])
            .args([
                "-o",
                "PreferredAuthentications=password,keyboard-interactive",
            ])
            .env("SSH_ASKPASS", self.askpass(PASSWORD))
            .env("SSH_ASKPASS_REQUIRE", "force");
        self.admin_through(ssh, script)
    }

    fn admin_through(&self, mut ssh: Command, script: &str) -> String {
        let output = ssh
            .args(self.connection_options())
            .arg("fixtureadmin@127.0.0.1")
            .arg(format!(
                "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
                encoded(&format!("$ErrorActionPreference='Stop';{script}"))
            ))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "the control connection failed: {}",
            text(&output.stderr)
        );
        text(&output.stdout)
    }

    fn connection_options(&self) -> Vec<String> {
        self.connection_options_trusting(&self.known_hosts)
    }

    fn connection_options_trusting(&self, known_hosts: &Path) -> Vec<String> {
        vec![
            "-p".into(),
            self.port.clone(),
            "-o".into(),
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            "IdentityAgent=none".into(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
            "-o".into(),
            "ConnectTimeout=10".into(),
        ]
    }

    /// The bytes of a guest file, or `None` when it does not exist.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let reply = self.admin(&format!(
            "if(Test-Path -LiteralPath '{path}'){{[Convert]::ToBase64String([IO.File]::ReadAllBytes('{path}'))}}else{{'-'}}"
        ));
        match reply.trim() {
            "-" => None,
            encoded => Some(decoded(encoded)),
        }
    }

    fn user_keys(&self) -> Option<String> {
        self.read(USER_KEYS)
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }

    /// Creates `fixtureuser`'s key file with `contents`, owned by that account.
    fn write_user_keys(&self, contents: &str) {
        self.admin(&format!(
            "$null=New-Item -ItemType Directory -Force C:\\Users\\fixtureuser\\.ssh;\
             [IO.File]::WriteAllBytes('{USER_KEYS}',[Convert]::FromBase64String('{}'));\
             icacls C:\\Users\\fixtureuser\\.ssh /setowner fixtureuser /t /q|Out-Null;'ok'",
            base64(contents.as_bytes())
        ));
    }

    fn key(&self, name: &str, comment: &str) -> PathBuf {
        let path = self.work.join(name);
        let made = Command::new(openssh("ssh-keygen.exe"))
            .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(made.status.success(), "ssh-keygen: {}", text(&made.stderr));
        path
    }

    fn askpass(&self, password: &str) -> PathBuf {
        let path = self.work.join(format!(
            "askpass-{}.cmd",
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::write(&path, format!("@echo {password}\r\n")).unwrap();
        path
    }

    fn copy_id(&self, key: &Path, user: &str) -> Output {
        self.copy_id_answering(PASSWORD, key, user, &self.known_hosts)
    }

    /// Runs the CLI for `user` with an askpass program answering `password`,
    /// trusting the host keys in `known_hosts`.
    fn copy_id_answering(
        &self,
        password: &str,
        key: &Path,
        user: &str,
        known_hosts: &Path,
    ) -> Output {
        self.copy_id_with(password, key, user, known_hosts, &[])
    }

    /// Runs the CLI as [`Guest::copy_id`] does, with `-t target`.
    fn copy_id_to(&self, key: &Path, user: &str, target: &str) -> Output {
        self.copy_id_with(PASSWORD, key, user, &self.known_hosts, &["-t", target])
    }

    fn copy_id_with(
        &self,
        password: &str,
        key: &Path,
        user: &str,
        known_hosts: &Path,
        options: &[&str],
    ) -> Output {
        let mut path =
            std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect::<Vec<_>>();
        path.insert(0, openssh(""));
        let child = Command::new(env!("CARGO_BIN_EXE_ssh-copy-id"))
            .args(options)
            .arg("-i")
            .arg(key)
            .args(["-F", "none"])
            .args(self.connection_options_trusting(known_hosts))
            .arg("-o")
            .arg(format!("IdentityFile=\"{}\"", key.display()))
            .arg(format!("{user}@127.0.0.1"))
            .env("USERPROFILE", self.work.join("home"))
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("SSH_ASKPASS", self.askpass(password))
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env_remove("SSH_AUTH_SOCK")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = wait_with_deadline(child, COPY_ID_TIMEOUT);
        let left: Vec<_> = fs::read_dir(self.work.join("home"))
            .unwrap()
            .flatten()
            .flat_map(|entry| fs::read_dir(entry.path()).into_iter().flatten().flatten())
            .map(|entry| entry.file_name())
            .collect();
        assert!(left.is_empty(), "scratch entries left: {left:?}");
        output
    }

    fn logs_in_with(&self, key: &Path, user: &str) -> bool {
        Command::new(openssh("ssh.exe"))
            .args(["-F", "none", "-i"])
            .arg(key)
            .args(self.connection_options())
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "PreferredAuthentications=publickey",
            ])
            .arg(format!("{user}@127.0.0.1"))
            .arg("exit")
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    }
}

impl Drop for Guest {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.work);
    }
}

fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(DIGITS[(bits >> (18 - 6 * i)) as usize & 63])
            } else {
                '='
            });
        }
    }
    out
}

fn decoded(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let mut bytes = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let digits: Vec<u8> = chunk
            .iter()
            .filter(|&&c| c != b'=')
            .map(|&c| value(c))
            .collect();
        let bits = digits
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &d)| acc | (u32::from(d) << (18 - 6 * i)));
        bytes.extend((0..digits.len() * 6 / 8).map(|i| (bits >> (16 - 8 * i)) as u8));
    }
    bytes
}

fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v01_requirement_2_installs_into_a_missing_file_and_the_key_logs_in() {
    let guest = Guest::start();
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(text(&run.stdout).contains("Number of key(s) added: 1"));
    assert!(
        text(&run.stderr).contains("the key authenticates: it is installed and verified"),
        "{}",
        text(&run.stderr)
    );
    assert!(
        !text(&run.stderr).contains("CLIXML"),
        "{}",
        text(&run.stderr)
    );
    assert_eq!(guest.user_keys().unwrap(), public_entry(&key));
    assert!(guest.logs_in_with(&key, "fixtureuser"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v02_requirement_2_an_empty_file_and_a_file_without_a_final_newline() {
    let guest = Guest::start();
    guest.write_user_keys("");
    let first = guest.key("first", "first@test");
    let run = guest.copy_id(&first, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(guest.user_keys().unwrap(), public_entry(&first));

    guest.write_user_keys("# existing");
    let second = guest.key("second", "second@test");
    let run = guest.copy_id(&second, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        guest.user_keys().unwrap(),
        format!("# existing\n{}", public_entry(&second))
    );
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v03_requirement_1_a_second_run_adds_nothing_and_another_key_is_not_taken_as_it() {
    let guest = Guest::start();
    let first = guest.key("first", "first@test");
    assert_eq!(guest.copy_id(&first, "fixtureuser").status.code(), Some(0));
    let again = guest.copy_id(&first, "fixtureuser");
    assert_eq!(again.status.code(), Some(0), "{}", text(&again.stderr));
    assert!(
        text(&again.stderr).contains("All keys were skipped"),
        "{}",
        text(&again.stderr)
    );
    assert_eq!(guest.user_keys().unwrap(), public_entry(&first));

    let second = guest.key("second", "second@test");
    let run = guest.copy_id(&second, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(
        guest.user_keys().unwrap(),
        format!("{}{}", public_entry(&first), public_entry(&second))
    );
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v04_requirement_3_an_unwritable_file_is_kept_and_reported() {
    let guest = Guest::start();
    guest.write_user_keys("# existing\n");
    guest.admin(&format!(
        "icacls '{USER_KEYS}' /deny 'fixtureuser:(W)' /q|Out-Null;'ok'"
    ));
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureuser");
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains("the key was not written to .ssh/authorized_keys"),
        "{}",
        text(&run.stderr)
    );
    assert!(!text(&run.stdout).contains("Number of key(s) added"));
    assert_eq!(guest.user_keys().unwrap(), "# existing\n");
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v05_requirement_7_comments_and_key_paths_with_spaces_japanese_and_quotes_are_data() {
    let guest = Guest::start();
    fs::create_dir_all(guest.work.join("鍵 'dir")).unwrap();
    let key = guest.key("鍵 'dir\\new key", "it's \"鍵\" %OS% $home");
    let run = guest.copy_id(&key, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(guest.user_keys().unwrap(), public_entry(&key));
    assert!(guest.logs_in_with(&key, "fixtureuser"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v10_a_key_line_past_the_size_where_sshd_withholds_the_end_of_input_installs() {
    let guest = Guest::start();
    let key = guest.key("long", &format!("long@{}", "c".repeat(5000)));
    let run = guest.copy_id(&key, "fixtureuser");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains("the key authenticates: it is installed and verified"),
        "{}",
        text(&run.stderr)
    );
    assert_eq!(guest.user_keys().unwrap(), public_entry(&key));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v06_requirement_8_a_wrong_password_and_a_host_key_mismatch_write_nothing() {
    let guest = Guest::start();
    let key = guest.key("new", "new@test");
    let wrong = guest.copy_id_answering("wrong-password", &key, "fixtureuser", &guest.known_hosts);
    assert_eq!(wrong.status.code(), Some(1), "{}", text(&wrong.stderr));
    assert!(!text(&wrong.stdout).contains("Number of key(s) added"));
    assert_eq!(guest.user_keys(), None);

    let other = guest.key("fake_host", "fake");
    let mismatched = guest.work.join("known_hosts");
    fs::write(
        &mismatched,
        format!(
            "[127.0.0.1]:{} {}",
            guest.port,
            fs::read_to_string(other.with_extension("pub")).unwrap()
        ),
    )
    .unwrap();
    let run = guest.copy_id_answering(PASSWORD, &key, "fixtureuser", &mismatched);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains("Host key verification failed"),
        "{}",
        text(&run.stderr)
    );
    assert_eq!(guest.user_keys(), None);
}

/// The shared administrator file's bytes and ACL, put back on drop through a
/// password login, since a test may remove the control key with the file.
struct SharedFile<'a> {
    guest: &'a Guest,
    bytes: Vec<u8>,
    sddl: String,
}

impl<'a> SharedFile<'a> {
    fn save(guest: &'a Guest) -> SharedFile<'a> {
        SharedFile {
            guest,
            bytes: guest
                .read(ADMIN_KEYS)
                .expect("the control key's shared file"),
            sddl: guest.shared_acl(),
        }
    }
}

impl Drop for SharedFile<'_> {
    fn drop(&mut self) {
        self.guest.admin_by_password(&format!(
            "[IO.File]::WriteAllBytes('{ADMIN_KEYS}',[Convert]::FromBase64String('{}'));\
             $s=New-Object Security.AccessControl.FileSecurity;\
             $s.SetSecurityDescriptorSddlForm('{}');\
             [IO.File]::SetAccessControl('{ADMIN_KEYS}',$s);'ok'",
            base64(&self.bytes),
            self.sddl
        ));
    }
}

impl Guest {
    /// The DACL of the shared administrator file, read through a password
    /// login.
    fn shared_acl(&self) -> String {
        self.admin_by_password(&format!(
            "[IO.File]::GetAccessControl('{ADMIN_KEYS}').GetSecurityDescriptorSddlForm('Access')"
        ))
        .trim()
        .to_string()
    }
}

const SHARED_NOTICE: &str = "ssh-copy-id: INFO: writing to \
     C:\\ProgramData\\ssh\\administrators_authorized_keys, which every administrator \
     account on this host shares";

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v07_an_administrator_s_key_is_appended_to_the_shared_file_whose_acl_is_kept() {
    let guest = Guest::start();
    let saved = SharedFile::save(&guest);
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureadmin");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    let stderr = text(&run.stderr);
    assert!(stderr.contains(SHARED_NOTICE), "{stderr}");
    assert!(
        stderr.contains("the key authenticates: it is installed and verified"),
        "{stderr}"
    );
    assert!(!stderr.contains("CLIXML"), "{stderr}");
    let mut expected = saved.bytes.clone();
    expected.extend_from_slice(public_entry(&key).as_bytes());
    assert_eq!(guest.read(ADMIN_KEYS).unwrap(), expected);
    assert_eq!(guest.shared_acl(), saved.sddl);
    assert!(guest.logs_in_with(&key, "fixtureadmin"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v08_a_missing_shared_file_is_created_for_administrators_and_system_only() {
    let guest = Guest::start();
    let _saved = SharedFile::save(&guest);
    guest.admin_by_password(&format!("Remove-Item -LiteralPath '{ADMIN_KEYS}';'ok'"));
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureadmin");
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains(SHARED_NOTICE),
        "{}",
        text(&run.stderr)
    );
    assert_eq!(
        guest
            .admin_by_password(&format!("[IO.File]::ReadAllText('{ADMIN_KEYS}')"))
            .trim_end(),
        public_entry(&key).trim_end()
    );
    assert_eq!(guest.shared_acl(), "D:P(A;;FA;;;SY)(A;;FA;;;BA)");
    assert!(guest.logs_in_with(&key, "fixtureadmin"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v09_an_administrator_s_ssh_session_holds_a_high_integrity_token() {
    let guest = Guest::start();
    let groups = guest.admin("whoami.exe /groups");
    assert!(groups.contains("S-1-16-12288"), "{groups}");
    assert!(!groups.contains("S-1-16-8192"), "{groups}");
}

/// A directory under `C:\`, where Authenticated Users may modify what
/// inherits from it, removed before and after the test.
const OUTSIDE: &str = r"C:\ssh-copy-id-w08";

struct Outside<'a>(&'a Guest);

impl<'a> Outside<'a> {
    fn clear(guest: &'a Guest) -> Outside<'a> {
        remove_outside(guest);
        Outside(guest)
    }
}

impl Drop for Outside<'_> {
    fn drop(&mut self) {
        remove_outside(self.0);
    }
}

fn remove_outside(guest: &Guest) {
    guest.admin(&format!(
        "if(Test-Path -LiteralPath '{OUTSIDE}'){{Remove-Item -Recurse -Force -LiteralPath '{OUTSIDE}'}};'ok'"
    ));
}

impl Guest {
    /// The access rules of a guest path, each as its SID, rights and type,
    /// sorted, and whether inheritance is removed.
    fn access_rules(&self, path: &str) -> (bool, Vec<String>) {
        let reply = self.admin(&format!(
            "$c=(Get-Item -LiteralPath '{path}').GetAccessControl('Access');\
             [Console]::Out.Write(\"$($c.AreAccessRulesProtected)`n\");\
             $c.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])|\
             %{{[Console]::Out.Write(\"$($_.IdentityReference) $($_.FileSystemRights) $($_.AccessControlType)`n\")}}"
        ));
        let mut lines = reply.lines();
        let protected = lines.next() == Some("True");
        let mut rules: Vec<String> = lines.map(str::to_string).collect();
        rules.sort();
        (protected, rules)
    }

    fn sid_of(&self, account: &str) -> String {
        self.admin(&format!(
            "(New-Object Security.Principal.NTAccount '{account}').Translate([Security.Principal.SecurityIdentifier]).Value"
        ))
        .trim()
        .to_string()
    }
}

#[test]
#[ignore = "needs the dockur Windows fixture whose sshd reads C:/ssh-copy-id-w08/keys for fixtureuser"]
fn v11_a_target_outside_the_profile_is_created_for_the_account_system_and_administrators() {
    let guest = Guest::start();
    let _outside = Outside::clear(&guest);
    let target = format!(r"{OUTSIDE}\keys");
    let key = guest.key("new", "new@test");
    let run = guest.copy_id_to(&key, "fixtureuser", &target);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains("the key authenticates: it is installed and verified"),
        "{}",
        text(&run.stderr)
    );
    let (protected, rules) = guest.access_rules(&target);
    assert!(protected);
    let mut expected = vec![
        format!("{} FullControl Allow", guest.sid_of("fixtureuser")),
        "S-1-5-18 FullControl Allow".to_string(),
        "S-1-5-32-544 FullControl Allow".to_string(),
    ];
    expected.sort();
    assert_eq!(rules, expected);
    let (protected, rules) = guest.access_rules(OUTSIDE);
    assert!(!protected);
    assert!(
        rules.iter().any(|rule| rule.starts_with("S-1-5-11 ")),
        "{rules:?}"
    );
    assert_eq!(
        String::from_utf8(guest.read(&target).unwrap()).unwrap(),
        public_entry(&key)
    );
    assert!(guest.logs_in_with(&key, "fixtureuser"));
}

#[test]
#[ignore = "needs the dockur Windows fixture whose sshd reads C:/ssh-copy-id-w08/keys for fixtureuser"]
fn v12_an_existing_target_inheriting_authenticated_users_keeps_its_acl_and_is_refused() {
    let guest = Guest::start();
    let _outside = Outside::clear(&guest);
    let target = format!(r"{OUTSIDE}\keys");
    guest.admin(&format!(
        "$null=New-Item -ItemType Directory '{OUTSIDE}';[IO.File]::WriteAllText('{target}','');'ok'"
    ));
    let before = guest.access_rules(&target);
    let key = guest.key("new", "new@test");
    let run = guest.copy_id_to(&key, "fixtureuser", &target);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert!(
        text(&run.stderr).contains(&format!(
            "check that the server's AuthorizedKeysFile setting reads {target}"
        )),
        "{}",
        text(&run.stderr)
    );
    assert_eq!(guest.access_rules(&target), before);
    assert_eq!(
        String::from_utf8(guest.read(&target).unwrap()).unwrap(),
        public_entry(&key)
    );
    assert!(!guest.logs_in_with(&key, "fixtureuser"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v13_an_administrator_s_target_is_the_profile_file_and_the_shared_file_is_untouched() {
    const ADMIN_PROFILE_KEYS: &str = r"C:\Users\fixtureadmin\.ssh\authorized_keys";
    let guest = Guest::start();
    let saved = SharedFile::save(&guest);
    let profile_keys = guest.read(ADMIN_PROFILE_KEYS);
    let key = guest.key("new", "new@test");
    let run = guest.copy_id_to(&key, "fixtureadmin", r".ssh\authorized_keys");
    let after = guest.read(ADMIN_PROFILE_KEYS);
    match &profile_keys {
        Some(bytes) => guest.admin(&format!(
            "[IO.File]::WriteAllBytes('{ADMIN_PROFILE_KEYS}',[Convert]::FromBase64String('{}'));'ok'",
            base64(bytes)
        )),
        None => guest.admin(&format!(
            "Remove-Item -LiteralPath '{ADMIN_PROFILE_KEYS}';'ok'"
        )),
    };
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    let stderr = text(&run.stderr);
    assert!(
        !stderr.contains("ssh-copy-id: INFO: writing to"),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            r"check that the server's AuthorizedKeysFile setting reads .ssh\authorized_keys"
        ),
        "{stderr}"
    );
    let mut expected = profile_keys.unwrap_or_default();
    expected.extend_from_slice(public_entry(&key).as_bytes());
    assert_eq!(after.unwrap(), expected);
    assert_eq!(guest.read(ADMIN_KEYS).unwrap(), saved.bytes);
}

impl Guest {
    /// Adds an entry allowing Users (`S-1-5-32-545`) `rights` on the shared
    /// administrator file, and returns its DACL afterwards.
    fn allow_users_on_shared_file(&self, rights: &str) -> String {
        self.admin_by_password(&format!(
            "$s=[IO.File]::GetAccessControl('{ADMIN_KEYS}');\
             $s.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule(\
             (New-Object Security.Principal.SecurityIdentifier 'S-1-5-32-545'),'{rights}','Allow')));\
             [IO.File]::SetAccessControl('{ADMIN_KEYS}',$s);'ok'"
        ));
        self.shared_acl()
    }

    /// The shared administrator file's bytes, read through a password login.
    fn shared_bytes(&self) -> Vec<u8> {
        decoded(
            self.admin_by_password(&format!(
                "[Convert]::ToBase64String([IO.File]::ReadAllBytes('{ADMIN_KEYS}'))"
            ))
            .trim(),
        )
    }
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v14_a_shared_file_users_may_write_is_appended_to_unchanged_and_refused() {
    let guest = Guest::start();
    let saved = SharedFile::save(&guest);
    let sddl = guest.allow_users_on_shared_file("Write");
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureadmin");
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains(SHARED_NOTICE), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "check that the server's AuthorizedKeysFile setting reads {ADMIN_KEYS} for administrators"
        )),
        "{stderr}"
    );
    assert_eq!(guest.shared_acl(), sddl);
    let mut expected = saved.bytes.clone();
    expected.extend_from_slice(public_entry(&key).as_bytes());
    assert_eq!(guest.shared_bytes(), expected);
    assert!(!guest.logs_in_with(&key, "fixtureadmin"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v15_a_shared_file_users_may_read_is_appended_to_unchanged_and_accepted() {
    let guest = Guest::start();
    let saved = SharedFile::save(&guest);
    let sddl = guest.allow_users_on_shared_file("Read");
    let key = guest.key("new", "new@test");
    let run = guest.copy_id(&key, "fixtureadmin");
    let stderr = text(&run.stderr);
    assert_eq!(run.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("the key authenticates: it is installed and verified"),
        "{stderr}"
    );
    assert_eq!(guest.shared_acl(), sddl);
    let mut expected = saved.bytes.clone();
    expected.extend_from_slice(public_entry(&key).as_bytes());
    assert_eq!(guest.shared_bytes(), expected);
    assert!(guest.logs_in_with(&key, "fixtureadmin"));
}

#[test]
#[ignore = "needs the dockur Windows fixture"]
fn v16_an_administrator_s_target_for_another_account_lets_that_account_log_in() {
    let guest = Guest::start();
    let key = guest.key("new", "new@test");
    let run = guest.copy_id_to(&key, "fixtureadmin", USER_KEYS);
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stderr));
    assert_eq!(guest.user_keys(), Some(public_entry(&key)));
    assert!(guest.logs_in_with(&key, "fixtureuser"));
}
