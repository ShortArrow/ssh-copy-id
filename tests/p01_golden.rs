//! P01 golden comparison: the pinned upstream script and this CLI run the same
//! scenarios against the L02 Linux fixture, and the report lists, per scenario,
//! both exit statuses, both normalized outputs, and both `authorized_keys` files.
//!
//! Step 1 asserts no parity. The report test fails only on harness errors: a
//! fixture that does not start, a pinned script whose SHA-256 differs, or a tool
//! that cannot start or does not finish. Each tool gets a fresh container per
//! scenario, the same arguments, and the same environment. Run it on Linux:
//!
//! ```text
//! docker build -t ssh-copy-id-l02:local tests/environments/linux
//! cargo test --test p01_golden -- --ignored --nocapture
//! ```
//!
//! The report is written to `<target dir>/p01-report.md` and printed.

mod common;

use common::{Fixture, keygen, public_line, text, wait_with_deadline};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const UPSTREAM_SCRIPT: &str = "tests/upstream/ssh-copy-id";
const UPSTREAM_SHA256: &str = "a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650";
const UPSTREAM_COMMIT: &str = "eabf1987de772f0f2d772fd6bd72b4c84d0ab780";
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
const USER: &str = "pwuser";

/// Text that differs between runs only because of where and how a run happened,
/// each replaced by a fixed placeholder.
struct Substitutions {
    /// Paths by which a tool was invoked, shown as `ssh-copy-id`.
    programs: Vec<String>,
    /// Identity file paths without `.pub`, shown as `<KEY>`.
    keys: Vec<String>,
    /// Per-run temporary directories, shown as `<TMP>`.
    temps: Vec<String>,
    /// The fixture's published port, shown as `<PORT>` where it stands alone.
    port: Option<String>,
}

/// Replaces program paths, then key paths, then temporary directories, then the
/// port, so that a path inside a temporary directory keeps its more specific
/// placeholder. The port is replaced only where no ASCII letter or digit adjoins it.
fn normalize(input: &str, with: &Substitutions) -> String {
    let mut out = input.to_string();
    for program in &with.programs {
        out = out.replace(program.as_str(), "ssh-copy-id");
    }
    for key in &with.keys {
        out = out.replace(key.as_str(), "<KEY>");
    }
    for temp in &with.temps {
        out = out.replace(temp.as_str(), "<TMP>");
    }
    match &with.port {
        Some(port) => replace_standalone(&out, port, "<PORT>"),
        None => out,
    }
}

fn replace_standalone(input: &str, needle: &str, placeholder: &str) -> String {
    let adjoins = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    for (at, _) in input.match_indices(needle) {
        let end = at + needle.len();
        if adjoins(input[..at].chars().last()) || adjoins(input[end..].chars().next()) {
            continue;
        }
        out.push_str(&input[copied..at]);
        out.push_str(placeholder);
        copied = end;
    }
    out.push_str(&input[copied..]);
    out
}

/// The SHA-256 digest of `bytes` as lowercase hex (FIPS 180-4).
fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    h.iter().fold(String::new(), |mut hex, word| {
        let _ = write!(hex, "{word:08x}");
        hex
    })
}

/// The pinned upstream script's bytes, or a panic naming both digests when the
/// committed file is not the pinned one.
fn pinned_upstream() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(UPSTREAM_SCRIPT);
    let bytes = fs::read(&path).unwrap();
    let actual = sha256_hex(&bytes);
    assert_eq!(
        actual,
        UPSTREAM_SHA256,
        "{} is not the pinned upstream script",
        path.display()
    );
    bytes
}

#[test]
fn n01_sha256_matches_the_standard_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn n02_the_committed_upstream_script_is_the_pinned_one() {
    pinned_upstream();
}

fn substitutions() -> Substitutions {
    Substitutions {
        programs: vec![
            "/tmp/p01/upstream/ssh-copy-id".into(),
            "/build/target/debug/ssh-copy-id".into(),
        ],
        keys: vec!["/tmp/p01/scenario-1/key".into()],
        temps: vec!["/tmp/p01/scenario-1".into(), "/tmp/l02-test-7".into()],
        port: Some("32768".into()),
    }
}

#[test]
fn n03_both_program_prefixes_become_the_bare_name() {
    let with = substitutions();
    assert_eq!(
        normalize(
            "/tmp/p01/upstream/ssh-copy-id: INFO: x\n/build/target/debug/ssh-copy-id: ERROR: y\nssh-copy-id: WARNING: z\n",
            &with
        ),
        "ssh-copy-id: INFO: x\nssh-copy-id: ERROR: y\nssh-copy-id: WARNING: z\n"
    );
}

#[test]
fn n04_the_program_path_in_a_usage_line_becomes_the_bare_name() {
    assert_eq!(
        normalize(
            "Usage: /tmp/p01/upstream/ssh-copy-id [-h|-?|-f]\n",
            &substitutions()
        ),
        "Usage: ssh-copy-id [-h|-?|-f]\n"
    );
}

#[test]
fn n05_key_paths_become_key_before_their_directory_becomes_tmp() {
    assert_eq!(
        normalize(
            "Source of key(s) to be installed: \"/tmp/p01/scenario-1/key.pub\"\n-i /tmp/p01/scenario-1/key\nUserKnownHostsFile=/tmp/l02-test-7/known_hosts /tmp/p01/scenario-1/other\n",
            &substitutions()
        ),
        "Source of key(s) to be installed: \"<KEY>.pub\"\n-i <KEY>\nUserKnownHostsFile=<TMP>/known_hosts <TMP>/other\n"
    );
}

#[test]
fn n06_the_port_is_replaced_only_where_it_stands_alone() {
    assert_eq!(
        normalize(
            "-p 32768 '[127.0.0.1]:32768' x32768 327689 AAAA32768\n32768",
            &substitutions()
        ),
        "-p <PORT> '[127.0.0.1]:<PORT>' x32768 327689 AAAA32768\n<PORT>"
    );
}

#[test]
fn n07_text_without_run_specific_parts_is_unchanged() {
    let text = "\nNumber of key(s) added: 1\n\n\tline with tab\r\n";
    assert_eq!(normalize(text, &substitutions()), text);
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Upstream,
    Cli,
}

#[derive(Clone, Copy)]
enum Arguments {
    Help,
    Nothing,
    UnknownOption,
    /// `-i` with the private key path, the fixture options, and pwuser.
    Install,
    /// `-i` with the `.pub` path, the fixture options, and pwuser.
    InstallNamingPublic,
}

#[derive(Clone, Copy)]
enum KeyFile {
    Generated,
    PrivateMissing,
    CommentAndBlankLineFirst,
}

#[derive(Clone, Copy)]
enum Password {
    Correct,
    Wrong,
}

#[derive(Clone, Copy)]
enum KnownHosts {
    Empty,
    Mismatch,
}

struct Scenario {
    title: &'static str,
    arguments: Arguments,
    key_file: KeyFile,
    password: Password,
    known_hosts: KnownHosts,
    /// A script run as pwuser in the fresh container before the tool runs.
    remote_setup: Option<&'static str>,
    /// Runs of the same tool against the same container before the recorded one.
    runs_before: usize,
}

const BASE: Scenario = Scenario {
    title: "",
    arguments: Arguments::Install,
    key_file: KeyFile::Generated,
    password: Password::Correct,
    known_hosts: KnownHosts::Empty,
    remote_setup: None,
    runs_before: 0,
};

const SCENARIOS: [Scenario; 10] = [
    Scenario {
        title: "-h (no destination)",
        arguments: Arguments::Help,
        ..BASE
    },
    Scenario {
        title: "no arguments",
        arguments: Arguments::Nothing,
        ..BASE
    },
    Scenario {
        title: "unknown option -z host",
        arguments: Arguments::UnknownOption,
        ..BASE
    },
    Scenario {
        title: "-i <pub> whose private key file is missing",
        arguments: Arguments::InstallNamingPublic,
        key_file: KeyFile::PrivateMissing,
        ..BASE
    },
    Scenario {
        title: "install into a missing authorized_keys with the password",
        ..BASE
    },
    Scenario {
        title: "second run after scenario 5 against the same container",
        runs_before: 1,
        ..BASE
    },
    Scenario {
        title: "wrong password",
        password: Password::Wrong,
        ..BASE
    },
    Scenario {
        title: "host key mismatch",
        known_hosts: KnownHosts::Mismatch,
        ..BASE
    },
    Scenario {
        title: "key file with a comment line and a blank line before the key",
        key_file: KeyFile::CommentAndBlankLineFirst,
        ..BASE
    },
    Scenario {
        title: "existing authorized_keys without a final newline",
        remote_setup: Some(
            "umask 077 && mkdir -p .ssh && printf 'existing' > .ssh/authorized_keys",
        ),
        ..BASE
    },
];

/// What one tool left behind in one scenario, normalized.
struct Outcome {
    arguments: String,
    status: Option<i32>,
    stdout: String,
    stderr: String,
    authorized_keys: Option<Vec<u8>>,
}

struct Harness {
    dir: PathBuf,
    upstream: PathBuf,
    cli: PathBuf,
}

impl Harness {
    /// Checks the pinned script and copies it into a temporary directory under the
    /// name `ssh-copy-id`, so that its `$0` is a path ending in that name.
    fn new() -> Harness {
        let bytes = pinned_upstream();
        let dir = std::env::temp_dir().join(format!("ssh-copy-id-p01-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("upstream")).unwrap();
        let upstream = dir.join("upstream").join("ssh-copy-id");
        fs::write(&upstream, bytes).unwrap();
        Harness {
            dir,
            upstream,
            cli: PathBuf::from(env!("CARGO_BIN_EXE_ssh-copy-id")),
        }
    }

    fn prepare_key(&self, number: usize, scenario: &Scenario) -> PathBuf {
        let dir = self.dir.join(format!("scenario-{number}"));
        fs::create_dir_all(&dir).unwrap();
        let key = keygen(&dir, "key", "p01@test");
        match scenario.key_file {
            KeyFile::Generated => {}
            KeyFile::PrivateMissing => fs::remove_file(&key).unwrap(),
            KeyFile::CommentAndBlankLineFirst => {
                let line = public_line(&key);
                fs::write(key.with_extension("pub"), format!("# comment\n\n{line}\n")).unwrap();
            }
        }
        key
    }

    fn outcome(&self, tool: Tool, scenario: &Scenario, key: &Path) -> Outcome {
        let fixture = Fixture::start();
        let home = fixture.work.join("home");
        fs::create_dir_all(home.join(".ssh")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(home.join(".ssh"), fs::Permissions::from_mode(0o700)).unwrap();
        }
        if let Some(script) = scenario.remote_setup {
            let setup = fixture.exec(USER, script);
            assert!(setup.status.success(), "{}", text(&setup.stderr));
        }
        if let KnownHosts::Mismatch = scenario.known_hosts {
            let other = keygen(&fixture.work, "fake_host", "fake");
            fs::write(
                fixture.known_hosts(),
                format!("[127.0.0.1]:{} {}\n", fixture.port, public_line(&other)),
            )
            .unwrap();
        }
        let password = match scenario.password {
            Password::Correct => fixture.password.clone(),
            Password::Wrong => "wrong-password".to_string(),
        };
        let askpass = fixture.askpass(&password);
        let arguments = arguments(scenario.arguments, &fixture, key);
        let run = || {
            let mut command = match tool {
                Tool::Upstream => {
                    let mut command = Command::new("sh");
                    command.arg(&self.upstream);
                    command
                }
                Tool::Cli => Command::new(&self.cli),
            };
            let child = command
                .args(&arguments)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", &home)
                .env("LC_ALL", "C")
                .env("SSH_ASKPASS", &askpass)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap_or_else(|error| {
                    panic!("cannot start {:?}: {error}", command.get_program())
                });
            wait_with_deadline(child, RUN_TIMEOUT)
        };
        for _ in 0..scenario.runs_before {
            run();
        }
        let output = run();
        let with = Substitutions {
            programs: vec![display(&self.upstream), display(&self.cli)],
            keys: vec![display(key)],
            temps: vec![
                display(key.parent().unwrap()),
                display(&fixture.work),
                display(&self.dir),
            ],
            port: Some(fixture.port.clone()),
        };
        Outcome {
            arguments: normalize(&shell_words(&arguments), &with),
            status: output.status.code(),
            stdout: normalize(&text(&output.stdout), &with),
            stderr: normalize(&text(&output.stderr), &with),
            authorized_keys: authorized_keys(&fixture),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn arguments(arguments: Arguments, fixture: &Fixture, key: &Path) -> Vec<String> {
    let identity = match arguments {
        Arguments::Help => return vec!["-h".into()],
        Arguments::Nothing => return vec![],
        Arguments::UnknownOption => return vec!["-z".into(), "host".into()],
        Arguments::Install => display(key),
        Arguments::InstallNamingPublic => display(&key.with_extension("pub")),
    };
    vec![
        "-i".into(),
        identity,
        "-p".into(),
        fixture.port.clone(),
        "-F".into(),
        "none".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", fixture.known_hosts().display()),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        "ConnectTimeout=5".into(),
        format!("{USER}@127.0.0.1"),
    ]
}

/// pwuser's `authorized_keys` read as root, or `None` when it does not exist.
fn authorized_keys(fixture: &Fixture) -> Option<Vec<u8>> {
    let path = format!("/home/{USER}/.ssh/authorized_keys");
    let read = fixture.exec_as_root(&format!("test -e {path} && cat {path}"));
    read.status.success().then_some(read.stdout)
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{}'", word.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn same(equal: bool) -> &'static str {
    if equal { "SAME" } else { "DIFF" }
}

fn status_text(status: Option<i32>) -> String {
    status.map_or_else(|| "killed by a signal".into(), |code| code.to_string())
}

fn block(report: &mut String, label: &str, body: &str) {
    if body.is_empty() {
        let _ = writeln!(report, "{label}: (empty)\n");
    } else {
        let _ = writeln!(report, "{label}:\n\n~~~text\n{body}\n~~~\n");
    }
}

fn keys_text(keys: &Option<Vec<u8>>) -> String {
    match keys {
        None => "(absent)".into(),
        Some(bytes) => format!("{:?}", text(bytes)),
    }
}

fn section(report: &mut String, number: usize, scenario: &Scenario, up: &Outcome, cli: &Outcome) {
    let _ = writeln!(report, "## {number}. {}\n", scenario.title);
    let _ = writeln!(report, "Arguments (both tools): `{}`\n", up.arguments);
    let _ = writeln!(
        report,
        "- exit status: {} (upstream {}, cli {})",
        same(up.status == cli.status),
        status_text(up.status),
        status_text(cli.status)
    );
    let _ = writeln!(report, "- stdout: {}", same(up.stdout == cli.stdout));
    let _ = writeln!(report, "- stderr: {}", same(up.stderr == cli.stderr));
    let _ = writeln!(
        report,
        "- authorized_keys: {} (upstream {}, cli {})\n",
        same(up.authorized_keys == cli.authorized_keys),
        keys_text(&up.authorized_keys),
        keys_text(&cli.authorized_keys)
    );
    block(report, "stdout, upstream", &up.stdout);
    block(report, "stdout, cli", &cli.stdout);
    block(report, "stderr, upstream", &up.stderr);
    block(report, "stderr, cli", &cli.stderr);
}

fn upstream_shell() -> String {
    let resolved = Command::new("sh")
        .args(["-c", "readlink -f \"$(command -v sh)\""])
        .output()
        .unwrap();
    text(&resolved.stdout).trim().to_string()
}

#[test]
#[ignore = "needs the L02 fixture image and a Linux client"]
fn p01_difference_report() {
    let harness = Harness::new();
    let mut report = String::new();
    let _ = writeln!(report, "# P01 difference report\n");
    let _ = writeln!(
        report,
        "Upstream: openssh-portable {UPSTREAM_COMMIT} contrib/ssh-copy-id, SHA-256 {UPSTREAM_SHA256}, run by `sh` ({}).\n",
        upstream_shell()
    );
    let mut summary = String::from(
        "| # | scenario | exit | stdout | stderr | authorized_keys |\n|---|---|---|---|---|---|\n",
    );
    let mut sections = String::new();
    for (index, scenario) in SCENARIOS.iter().enumerate() {
        let number = index + 1;
        let key = harness.prepare_key(number, scenario);
        let up = harness.outcome(Tool::Upstream, scenario, &key);
        let cli = harness.outcome(Tool::Cli, scenario, &key);
        let _ = writeln!(
            summary,
            "| {number} | {} | {} | {} | {} | {} |",
            scenario.title,
            same(up.status == cli.status),
            same(up.stdout == cli.stdout),
            same(up.stderr == cli.stderr),
            same(up.authorized_keys == cli.authorized_keys)
        );
        section(&mut sections, number, scenario, &up, &cli);
    }
    let _ = writeln!(report, "{summary}\n{sections}");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap()
        .join("p01-report.md");
    fs::write(&path, &report).unwrap();
    println!("{report}");
    println!("report written to {}", path.display());
}
