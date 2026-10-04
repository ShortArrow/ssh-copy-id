//! P01 golden comparison: the pinned upstream script and this CLI run the same
//! scenarios against the L02 Linux fixture, and the report lists, per scenario,
//! both exit statuses, both normalized outputs, and both `authorized_keys` files.
//!
//! The report test asserts parity: per scenario, the exit statuses, both
//! outputs and both `authorized_keys` files are equal once the differences in
//! `EXPECTED` are taken out, and every difference marked `always` there appears.
//! It also fails on harness errors: a fixture that does not start, a pinned
//! script whose SHA-256 differs, or a tool that cannot start or does not finish.
//! The report is printed and written first. Each tool gets a fresh container per
//! scenario, the same arguments, and the same environment. Run it on Linux:
//!
//! ```text
//! docker build -t ssh-copy-id-l02:local tests/environments/linux
//! cargo test --test p01_golden -- --ignored --nocapture
//! ```
//!
//! The report is written to `<target dir>/p01-report.md` and printed.

mod common;

use common::{Agent, Fixture, keygen, public_line, text, wait_with_deadline};
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

const D19_LINE: &str = "ssh-copy-id: ERROR: ssh exited with status 255 before the installation script reported anything; if authentication failed, nothing was written\n";
const D06_LINE: &str = "ssh-copy-id: INFO: the key authenticates: it is installed and verified\n";
const D13_LINE: &str = "ssh-copy-id: WARNING: OpenSSH_10.3p1, OpenSSL 3.6.3 9 Jun 2026 has not been tested with this release; output it cannot classify makes the installed-key check inconclusive\n";

fn outcome(stdout: &str, stderr: &str) -> Outcome {
    Outcome {
        arguments: String::new(),
        status: Some(0),
        stdout: stdout.into(),
        stderr: stderr.into(),
        authorized_keys: None,
    }
}

#[test]
fn n08_an_expected_difference_is_removed() {
    let reconciled = without_expected(&format!("a\n{D19_LINE}"), 7, Field::Stderr, Tool::Cli);
    assert_eq!(reconciled.text, "a\n");
    assert!(reconciled.missing.is_empty(), "{:?}", reconciled.missing);
}

#[test]
fn n09_a_required_difference_that_is_absent_is_named_by_its_tag() {
    let reconciled = without_expected("a\n", 7, Field::Stderr, Tool::Cli);
    assert_eq!(reconciled.missing, ["D-19"]);
}

#[test]
fn n10_the_untested_client_warning_may_be_absent() {
    let with = without_expected(
        &format!("{D13_LINE}{D06_LINE}"),
        5,
        Field::Stderr,
        Tool::Cli,
    );
    let without = without_expected(D06_LINE, 5, Field::Stderr, Tool::Cli);
    assert_eq!(with.text, "");
    assert_eq!(without.text, "");
    assert!(without.missing.is_empty(), "{:?}", without.missing);
}

#[test]
fn n11_either_shell_s_getopts_line_from_upstream_becomes_bash_s() {
    for line in ["ssh-copy-id: illegal option -- z\n", "Illegal option -z\n"] {
        let reconciled = without_expected(
            &format!("{line}{UPSTREAM_USAGE}"),
            3,
            Field::Stderr,
            Tool::Upstream,
        );
        assert_eq!(
            reconciled.text, "ssh-copy-id: illegal option -- z\n",
            "{line:?}"
        );
        assert!(reconciled.missing.is_empty(), "{:?}", reconciled.missing);
    }
}

#[test]
fn n12_the_cli_must_print_bash_s_getopts_line() {
    let up = outcome("", &format!("Illegal option -z\n{UPSTREAM_USAGE}"));
    let cli = outcome("", &format!("Illegal option -z\n{CLI_USAGE}"));
    assert_eq!(unexpected_differences(3, &up, &cli), ["stderr"]);
}

#[test]
fn n13_a_difference_outside_the_table_is_named() {
    let up = outcome("x\n", UPSTREAM_USAGE);
    let cli = outcome("y\n", CLI_USAGE);
    assert_eq!(unexpected_differences(1, &up, &cli), ["stdout"]);
}

#[test]
fn n15_exit_status_and_authorized_keys_differ_only_where_the_table_says() {
    let mut up = outcome("", "");
    let mut cli = outcome("", "");
    up.authorized_keys = Some(b"k\n".to_vec());
    cli.status = Some(1);
    assert_eq!(
        unexpected_differences(8, &up, &cli),
        ["exit status", "authorized_keys"]
    );
}

#[test]
fn n16_a_whole_replacement_applies_only_when_the_text_differs() {
    assert_eq!(apply(&Edit::Whole("x"), "y").as_deref(), Some("x"));
    assert_eq!(apply(&Edit::Whole("x"), "x"), None);
}

#[test]
fn n14_every_scenario_lists_a_rule_only_for_scenarios_that_exist() {
    for rule in &EXPECTED {
        for number in rule.scenarios {
            assert!(
                (1..=SCENARIOS.len()).contains(number),
                "{} names scenario {number}",
                rule.tag
            );
        }
    }
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
    /// The fixture options and pwuser, without `-i`.
    DefaultKey,
    /// `-i` without a file, the fixture options, and pwuser.
    IdentityWithoutFile,
    /// `-f`, then `-i` with the private key path, the fixture options, and pwuser.
    ForceInstall,
    /// `-f`, then `-i` with the `.pub` path, the fixture options, and pwuser.
    ForceInstallNamingPublic,
    /// `-i` with the `.pub` path, then `-f`, the fixture options, and pwuser.
    InstallNamingPublicThenForce,
}

#[derive(Clone, Copy)]
enum KeyFile {
    Generated,
    PrivateMissing,
    CommentAndBlankLineFirst,
    /// The generated key's line followed by the line of another generated key.
    TwoKeys,
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
    /// Whether the local `~/.ssh` holds the scenario's key as `id_ed25519`.
    default_key_file: bool,
    /// Whether a private agent holds the scenario's key and a second key, and
    /// pwuser's `authorized_keys` already holds the scenario's key.
    agent: bool,
}

const BASE: Scenario = Scenario {
    title: "",
    arguments: Arguments::Install,
    key_file: KeyFile::Generated,
    password: Password::Correct,
    known_hosts: KnownHosts::Empty,
    remote_setup: None,
    runs_before: 0,
    default_key_file: false,
    agent: false,
};

const SCENARIOS: [Scenario; 21] = [
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
    Scenario {
        title: "-i file holding two keys",
        key_file: KeyFile::TwoKeys,
        ..BASE
    },
    Scenario {
        title: "no -i: the default key file",
        arguments: Arguments::DefaultKey,
        default_key_file: true,
        ..BASE
    },
    Scenario {
        title: "-i without a file: the default key file",
        arguments: Arguments::IdentityWithoutFile,
        default_key_file: true,
        ..BASE
    },
    Scenario {
        title: "no -i and no default key file",
        arguments: Arguments::DefaultKey,
        ..BASE
    },
    Scenario {
        title: "-i without a file and no default key file",
        arguments: Arguments::IdentityWithoutFile,
        ..BASE
    },
    Scenario {
        title: "no -i: two agent keys, the first already installed",
        arguments: Arguments::DefaultKey,
        agent: true,
        ..BASE
    },
    Scenario {
        title: "-f: install into a missing authorized_keys",
        arguments: Arguments::ForceInstall,
        ..BASE
    },
    Scenario {
        title: "-f: second run after a -f run, adding the key again",
        arguments: Arguments::ForceInstall,
        runs_before: 1,
        ..BASE
    },
    Scenario {
        title: "-f -i <pub> whose private key file is missing",
        arguments: Arguments::ForceInstallNamingPublic,
        key_file: KeyFile::PrivateMissing,
        ..BASE
    },
    Scenario {
        title: "-i <pub> -f: -f after -i, with the private key file",
        arguments: Arguments::InstallNamingPublicThenForce,
        ..BASE
    },
    Scenario {
        title: "-i <pub> -f: -f after -i, whose private key file is missing",
        arguments: Arguments::InstallNamingPublicThenForce,
        key_file: KeyFile::PrivateMissing,
        ..BASE
    },
];

/// Upstream's usage, which the CLI's matches only from stage 1.5.
const UPSTREAM_USAGE: &str = "Usage: ssh-copy-id [-h|-?|-f|-n|-s|-x] [-i [identity_file]] [-t target_path] [-F ssh_config] [[-o ssh_option] ...] [-p port] [user@]hostname
\t-f: force mode -- copy keys without trying to check if they are already installed
\t-n: dry run    -- no keys are actually copied
\t-s: use sftp   -- use sftp instead of executing remote-commands. Can be useful if the remote only allows sftp
\t-x: debug      -- enables -x in this shell, for debugging
\t-h|-?: print this help
";

/// The CLI's usage.
const CLI_USAGE: &str = "Usage: ssh-copy-id [-h|-?|-f] [-i [identity_file]] [-p port] [-F ssh_config] [[-o ssh_option] ...] [user@]hostname
\t-f: force mode -- copy keys without trying to check if they are already installed
\t-i: the public key to install; '.pub' is added when absent
\t-p: port of the remote host
\t-F, -o: passed to ssh unchanged
\t-h|-?: print this help
This release installs keys on a Unix-like host.
-n, -s, -t, and -x are not available yet.
";

#[derive(Clone, Copy, PartialEq)]
enum Field {
    /// The exit status as the report shows it.
    ExitStatus,
    Stdout,
    Stderr,
    /// pwuser's `authorized_keys` as the report shows it.
    AuthorizedKeys,
}

/// How an expected difference is taken out of one tool's output.
enum Edit {
    /// Removes the first occurrence of this text.
    Remove(&'static str),
    /// Removes every line that starts with the first text and ends with the
    /// second, its newline included.
    RemoveLine(&'static str, &'static str),
    /// Replaces the first occurrence of the first text with the second.
    Replace(&'static str, &'static str),
    /// Replaces the whole text, for an outcome the design prevents altogether.
    Whole(&'static str),
}

/// A difference between the tools that the design expects, tagged with the
/// reason: a recorded difference (`D-nn`), the options stage 1.5 adds
/// (`STAGE-1.5`), or the shell that runs upstream (`SHELL`).
struct Expected {
    tag: &'static str,
    scenarios: &'static [usize],
    field: Field,
    tool: Tool,
    edit: Edit,
    /// Whether the difference appears in every listed scenario on every
    /// machine; a missing one fails the test.
    always: bool,
}

const EXPECTED: [Expected; 15] = [
    Expected {
        tag: "D-06",
        scenarios: &[5, 9, 10, 12, 13],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::Remove(
            "ssh-copy-id: INFO: the key authenticates: it is installed and verified\n",
        ),
        always: true,
    },
    Expected {
        tag: "D-06",
        scenarios: &[16],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::Remove(
            "ssh-copy-id: INFO: key 2 from ssh-add -L: the key authenticates: it is installed and verified\n",
        ),
        always: true,
    },
    Expected {
        tag: "D-13",
        scenarios: &[5, 6, 7, 8, 9, 10, 12, 13, 16],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::RemoveLine(
            "ssh-copy-id: WARNING: ",
            " has not been tested with this release; output it cannot classify makes the installed-key check inconclusive",
        ),
        always: false,
    },
    Expected {
        tag: "D-18",
        scenarios: &[9],
        field: Field::Stderr,
        tool: Tool::Upstream,
        edit: Edit::Replace(
            "ssh-copy-id: INFO: 3 key(s) remain to be installed",
            "ssh-copy-id: INFO: 1 key(s) remain to be installed",
        ),
        always: true,
    },
    Expected {
        tag: "D-18",
        scenarios: &[9],
        field: Field::Stdout,
        tool: Tool::Upstream,
        edit: Edit::Replace("Number of key(s) added: 3\n", "Number of key(s) added: 1\n"),
        always: true,
    },
    Expected {
        tag: "D-19",
        scenarios: &[7],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::Remove(
            "ssh-copy-id: ERROR: ssh exited with status 255 before the installation script reported anything; if authentication failed, nothing was written\n",
        ),
        always: true,
    },
    Expected {
        tag: "D-21",
        scenarios: &[11],
        field: Field::ExitStatus,
        tool: Tool::Upstream,
        edit: Edit::Replace("0", "1"),
        always: true,
    },
    Expected {
        tag: "D-21",
        scenarios: &[11],
        field: Field::Stdout,
        tool: Tool::Upstream,
        edit: Edit::Whole(""),
        always: true,
    },
    Expected {
        tag: "D-21",
        scenarios: &[11],
        field: Field::Stderr,
        tool: Tool::Upstream,
        edit: Edit::Whole(""),
        always: true,
    },
    Expected {
        tag: "D-21",
        scenarios: &[11],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::Remove(
            "ssh-copy-id: ERROR: '<KEY>.pub' holds 2 keys; a selected key file must hold one key, the public half of '<KEY>'\n",
        ),
        always: true,
    },
    Expected {
        tag: "D-21",
        scenarios: &[11],
        field: Field::AuthorizedKeys,
        tool: Tool::Upstream,
        edit: Edit::Whole("(absent)"),
        always: true,
    },
    Expected {
        tag: "STAGE-1.5",
        scenarios: &[1, 2, 3],
        field: Field::Stderr,
        tool: Tool::Upstream,
        edit: Edit::Remove(UPSTREAM_USAGE),
        always: true,
    },
    Expected {
        tag: "STAGE-1.5",
        scenarios: &[1, 2, 3],
        field: Field::Stderr,
        tool: Tool::Cli,
        edit: Edit::Remove(CLI_USAGE),
        always: true,
    },
    Expected {
        tag: "SHELL",
        scenarios: &[3],
        field: Field::Stderr,
        tool: Tool::Upstream,
        edit: Edit::Replace("Illegal option -z\n", "ssh-copy-id: illegal option -- z\n"),
        always: false,
    },
    Expected {
        tag: "SHELL",
        scenarios: &[4, 21],
        field: Field::Stderr,
        tool: Tool::Upstream,
        edit: Edit::Replace("': No such file\n", "': No such file or directory\n"),
        always: false,
    },
];

/// One tool's output with the expected differences taken out, and the tags of
/// the differences marked `always` that were not found.
struct Reconciled {
    text: String,
    missing: Vec<&'static str>,
}

fn apply(edit: &Edit, text: &str) -> Option<String> {
    match *edit {
        Edit::Remove(removed) => text
            .contains(removed)
            .then(|| text.replacen(removed, "", 1)),
        Edit::Replace(from, to) => text.contains(from).then(|| text.replacen(from, to, 1)),
        Edit::Whole(to) => (text != to).then(|| to.to_string()),
        Edit::RemoveLine(starts, ends) => {
            let kept: String = text
                .split_inclusive('\n')
                .filter(|line| {
                    let body = line.strip_suffix('\n').unwrap_or(line);
                    !(body.starts_with(starts) && body.ends_with(ends))
                })
                .collect();
            (kept != text).then_some(kept)
        }
    }
}

fn without_expected(text: &str, number: usize, field: Field, tool: Tool) -> Reconciled {
    let mut reconciled = Reconciled {
        text: text.to_string(),
        missing: Vec::new(),
    };
    for rule in EXPECTED
        .iter()
        .filter(|rule| rule.scenarios.contains(&number) && rule.field == field && rule.tool == tool)
    {
        match apply(&rule.edit, &reconciled.text) {
            Some(text) => reconciled.text = text,
            None if rule.always => reconciled.missing.push(rule.tag),
            None => {}
        }
    }
    reconciled
}

/// The fields of one scenario that differ once the expected differences are
/// taken out, each named, with `missing <tag>` for an expected difference
/// that did not appear.
fn unexpected_differences(number: usize, up: &Outcome, cli: &Outcome) -> Vec<String> {
    let mut found = Vec::new();
    for (name, field, up_text, cli_text) in [
        (
            "exit status",
            Field::ExitStatus,
            &status_text(up.status),
            &status_text(cli.status),
        ),
        ("stdout", Field::Stdout, &up.stdout, &cli.stdout),
        ("stderr", Field::Stderr, &up.stderr, &cli.stderr),
        (
            "authorized_keys",
            Field::AuthorizedKeys,
            &keys_text(&up.authorized_keys),
            &keys_text(&cli.authorized_keys),
        ),
    ] {
        let up_side = without_expected(up_text, number, field, Tool::Upstream);
        let cli_side = without_expected(cli_text, number, field, Tool::Cli);
        if up_side.text != cli_side.text {
            found.push(name.to_string());
        }
        for tag in up_side.missing.iter().chain(&cli_side.missing) {
            found.push(format!("{name}: missing {tag}"));
        }
    }
    found
}

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
            KeyFile::TwoKeys => {
                let other = keygen(&dir, "other", "other@test");
                let lines = format!("{}\n{}\n", public_line(&key), public_line(&other));
                fs::write(key.with_extension("pub"), lines).unwrap();
            }
        }
        if scenario.agent {
            keygen(&dir, "other", "other@test");
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
        if scenario.default_key_file {
            let default = home.join(".ssh").join("id_ed25519");
            fs::copy(key, &default).unwrap();
            fs::copy(key.with_extension("pub"), default.with_extension("pub")).unwrap();
        }
        if let Some(script) = scenario.remote_setup {
            let setup = fixture.exec(USER, script);
            assert!(setup.status.success(), "{}", text(&setup.stderr));
        }
        let agent = scenario.agent.then(|| {
            let agent = Agent::start(&fixture.work);
            agent.add(key);
            agent.add(&key.with_file_name("other"));
            let setup = fixture.exec(
                USER,
                &format!(
                    "umask 077 && mkdir -p .ssh && printf '%s\\n' '{}' > .ssh/authorized_keys",
                    public_line(key)
                ),
            );
            assert!(setup.status.success(), "{}", text(&setup.stderr));
            agent
        });
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
                .envs(agent.iter().map(|agent| ("SSH_AUTH_SOCK", &agent.socket)))
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
    let identity: Vec<String> = match arguments {
        Arguments::Help => return vec!["-h".into()],
        Arguments::Nothing => return vec![],
        Arguments::UnknownOption => return vec!["-z".into(), "host".into()],
        Arguments::Install => vec!["-i".into(), display(key)],
        Arguments::InstallNamingPublic => vec!["-i".into(), display(&key.with_extension("pub"))],
        Arguments::DefaultKey => vec![],
        Arguments::IdentityWithoutFile => vec!["-i".into()],
        Arguments::ForceInstall => vec!["-f".into(), "-i".into(), display(key)],
        Arguments::ForceInstallNamingPublic => vec![
            "-f".into(),
            "-i".into(),
            display(&key.with_extension("pub")),
        ],
        Arguments::InstallNamingPublicThenForce => vec![
            "-i".into(),
            display(&key.with_extension("pub")),
            "-f".into(),
        ],
    };
    let mut words = identity;
    words.extend([
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
    ]);
    words
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
        "| # | scenario | exit | stdout | stderr | authorized_keys | expected differences | unexpected |\n|---|---|---|---|---|---|---|---|\n",
    );
    let mut sections = String::new();
    let mut failures = Vec::new();
    for (index, scenario) in SCENARIOS.iter().enumerate() {
        let number = index + 1;
        let key = harness.prepare_key(number, scenario);
        let up = harness.outcome(Tool::Upstream, scenario, &key);
        let cli = harness.outcome(Tool::Cli, scenario, &key);
        let unexpected = unexpected_differences(number, &up, &cli);
        let _ = writeln!(
            summary,
            "| {number} | {} | {} | {} | {} | {} | {} | {} |",
            scenario.title,
            same(up.status == cli.status),
            same(up.stdout == cli.stdout),
            same(up.stderr == cli.stderr),
            same(up.authorized_keys == cli.authorized_keys),
            expected_tags(number),
            if unexpected.is_empty() {
                "none".to_string()
            } else {
                unexpected.join(", ")
            }
        );
        section(&mut sections, number, scenario, &up, &cli);
        failures.extend(
            unexpected
                .into_iter()
                .map(|difference| format!("scenario {number}: {difference}")),
        );
    }
    let _ = writeln!(report, "{summary}\n{sections}");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap()
        .join("p01-report.md");
    fs::write(&path, &report).unwrap();
    println!("{report}");
    println!("report written to {}", path.display());
    assert!(
        failures.is_empty(),
        "differences not in EXPECTED:\n{}",
        failures.join("\n")
    );
}

/// The tags of the expected differences listed for a scenario, in table order
/// without repeats, or `none`.
fn expected_tags(number: usize) -> String {
    let mut tags: Vec<&str> = Vec::new();
    for rule in EXPECTED
        .iter()
        .filter(|rule| rule.scenarios.contains(&number))
    {
        if !tags.contains(&rule.tag) {
            tags.push(rule.tag);
        }
    }
    if tags.is_empty() {
        "none".to_string()
    } else {
        tags.join(", ")
    }
}
