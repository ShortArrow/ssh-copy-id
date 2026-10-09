//! The installed-key check: which identities could answer the probe, and what the probe's result means.

use crate::key_input::key_lines;
use std::path::{Path, PathBuf};

/// The result of checking whether the selected key is already installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The selected key was the only candidate and the server accepted it,
    /// alone or with partial success.
    Installed,
    /// The server rejected public-key authentication.
    NotInstalled,
    /// The probe cannot tell; the key is installed with this reason as a warning.
    Inconclusive(String),
    /// Authentication was never attempted: the host key was rejected or the
    /// connection failed, and `ssh` exited with status 255. The run stops.
    /// `failure` is the line naming the failure; `messages` are `ssh`'s lines,
    /// those of the log and then those of stderr, each without its `\n` but
    /// with any `\r`, leaving out the lines that only `LogLevel=VERBOSE` writes,
    /// so that they are what upstream's `LogLevel=INFO` probe prints.
    NotAttempted {
        failure: String,
        messages: Vec<String>,
    },
    /// The probe could not reach a verdict about authentication for another
    /// reason; the run stops with this message.
    Failed(String),
}

/// Client version strings, as `ssh -V` prints them, that the fixtures have tested:
/// Windows OpenSSH, Git for Windows, and Ubuntu 24.04. Each is a prefix of the
/// first line of `ssh -V`, so an Ubuntu point release still matches while an
/// unpatched `OpenSSH_9.6p1` does not.
pub const TESTED_CLIENTS: [&str; 3] = [
    "OpenSSH_for_Windows_9.5p2",
    "OpenSSH_10.0p2",
    "OpenSSH_9.6p1 Ubuntu-3ubuntu13",
];

/// Lists the identities other than `selected` that `ssh` could offer, read from `ssh -G` output.
///
/// `selected` is the `-i` argument exactly as passed to `ssh`. `home` expands a
/// leading `~/`. `exists` reports whether a file is present; absent identity and
/// certificate files are not offered by `ssh` and are not counted. `same_file`
/// reports whether two paths name the same file; an `identityfile` value that
/// equals `selected` textually or names the same file is not a candidate.
/// Any other `identityfile` is a candidate when the file or its public half,
/// the value with `.pub` appended, exists: `ssh` loads the public half and
/// offers the agent key that matches it even with `IdentitiesOnly=yes`.
/// A value containing `%` or `${` cannot be expanded here and is always a
/// candidate, without consulting `exists`. The selected key's own certificate,
/// `<selected>-cert.pub` or `<selected>-cert`, is a candidate when it exists.
pub fn other_candidates_matching(
    config: &str,
    selected: &str,
    home: &Path,
    exists: &dyn Fn(&Path) -> bool,
    same_file: &dyn Fn(&Path, &Path) -> bool,
) -> Vec<String> {
    let is_selected = |value: &str| {
        value == selected
            || (is_expandable(value) && same_file(&expand(home, value), Path::new(selected)))
    };
    let is_present = |value: &str| !is_expandable(value) || exists(&expand(home, value));
    let is_identity_present =
        |value: &str| is_present(value) || is_present(&format!("{value}.pub"));
    let mut others = Vec::new();
    for line in config.lines() {
        let line = line.trim_end_matches('\r');
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "identityfile" if !is_selected(value) && is_identity_present(value) => {
                others.push(format!("identity file {value}"));
            }
            "certificatefile" if is_present(value) => {
                others.push(format!("certificate {value}"));
            }
            "pkcs11provider" if value != "none" => {
                others.push(format!("PKCS#11 provider {value}"));
            }
            _ => {}
        }
    }
    let own_certificate = ["-cert.pub", "-cert"]
        .iter()
        .map(|suffix| format!("{selected}{suffix}"))
        .find(|path| exists(&expand(home, path)));
    if let Some(own_certificate) = own_certificate {
        others.push(format!("certificate {own_certificate}"));
    }
    others
}

fn is_expandable(value: &str) -> bool {
    !value.contains('%') && !value.contains("${")
}

fn expand(home: &Path, value: &str) -> PathBuf {
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    }
}

/// Fragments of a status-255 probe's output that mean the host key was rejected
/// or the connection failed, so authentication was never attempted.
const NO_AUTHENTICATION_FAILURES: [&str; 12] = [
    "Host key verification failed",
    "REMOTE HOST IDENTIFICATION HAS CHANGED",
    "Connection refused",
    "Connection timed out",
    "Operation timed out",
    "Could not resolve hostname",
    "No route to host",
    "Network is unreachable",
    "Connection closed by",
    "Connection reset",
    "kex_exchange_identification",
    "connect to host",
];

/// Classifies a probe from its exit status, the log `ssh` wrote with `-E`, and
/// its stderr, given the other candidates.
///
/// `ssh` writes its own messages to the log, while stderr carries the server's
/// banner and remote output, so nothing on stderr is evidence of `Installed`.
/// Any exit status other than 255 is `Installed` when no other candidate
/// exists and the first `Authenticated to` line of the log names `publickey`
/// as `using "publickey"` (the probe runs with `LogLevel=VERBOSE`); a key with
/// `command=` makes the probe exit nonzero after authenticating. Later
/// `Authenticated to` lines and `using "publickey"` elsewhere are not
/// evidence. Exit 255 is never `Installed`: `ssh` writes a server's disconnect
/// message, which may span lines, into the log before verifying the host key,
/// and a disconnect always ends `ssh` with 255. A server accepting another
/// method, such as `none`, lets the probe succeed without the key. Exit 0 with
/// no `Authenticated to` line in the log is `Failed`, since `ssh` exits 0 only after authenticating. A process
/// terminated by a signal is `Failed`. Exit 255 is `NotInstalled` on
/// `Permission denied (`, `NotAttempted` naming the last line with a recognised
/// host key or connection failure, and `Inconclusive` otherwise; these patterns
/// are matched on the log and stderr together.
///
/// One exception to exit 255: when the first line of the log that reports an
/// authentication is `Authenticated using "publickey" with partial success.`,
/// the server accepted the key and requires a further method, which the probe
/// does not offer, so `ssh` exits 255 with `Permission denied (`. That is
/// `Installed` without other candidates and `Inconclusive` with them, whatever
/// the exit status. A log with a `Received disconnect from ` line is not
/// evidence of it, since the server's disconnect message may have written it.
pub fn classify(exit: Option<i32>, log: &str, stderr: &str, others: &[String]) -> CheckResult {
    let output = format!("{log}\n{stderr}");
    match exit {
        None => CheckResult::Failed("ssh was terminated before it reported a result".to_string()),
        Some(_) if accepted_with_partial_success(log) && others.is_empty() => {
            CheckResult::Installed
        }
        Some(_) if accepted_with_partial_success(log) => another_identity(others),
        Some(code)
            if code != 255
                && others.is_empty()
                && authenticated_method(log) == Some("publickey") =>
        {
            CheckResult::Installed
        }
        Some(0) if !others.is_empty() => another_identity(others),
        Some(0) => match authenticated_method(log) {
            Some(method) => CheckResult::Inconclusive(format!(
                "authentication succeeded without the selected key, using \"{method}\""
            )),
            None => {
                CheckResult::Failed("ssh exited without recording an authentication".to_string())
            }
        },
        Some(255) if output.contains("Permission denied (") => CheckResult::NotInstalled,
        Some(255) => match (failure_line(&output), last_line(&output)) {
            (Some(line), _) => CheckResult::NotAttempted {
                failure: line,
                messages: relayed_messages(log, stderr),
            },
            (None, Some(line)) => {
                CheckResult::Inconclusive(format!("ssh exited with status 255: {line}"))
            }
            (None, None) => CheckResult::Inconclusive("ssh exited with status 255".to_string()),
        },
        Some(code) => CheckResult::Inconclusive(format!(
            "the session failed after authentication with exit status {code}"
        )),
    }
}

/// The starts of the messages the OpenSSH client logs at `VERBOSE`, the level
/// above upstream's `LogLevel=INFO`, from the call sites a probe can reach:
/// `sshconnect2.c` (authentication), `clientloop.c` (session end), `ssh.c`
/// (hostname canonicalization), `kex.c` (identification exchange) and
/// `hostfile.c` (`known_hosts` parsing). The log carries no level, so a line is
/// recognised by how it starts.
const VERBOSE_ONLY_MESSAGES: [&str; 11] = [
    "Authenticated to ",
    "Authenticated using \"",
    "Transferred: sent ",
    "Bytes per second: sent ",
    "Killed by signal ",
    "Canonicalized DNS aliased hostname ",
    "kex_exchange_identification: Connection closed by remote host",
    "kex_exchange_identification: banner line contains invalid characters",
    "kex_exchange_identification: banner line too long",
    "hostkeys_foreach_file: invalid marker at ",
    "hostkeys_foreach_file: truncated line at ",
];

fn relayed_messages(log: &str, stderr: &str) -> Vec<String> {
    log.split_terminator('\n')
        .chain(stderr.split_terminator('\n'))
        .filter(|line| {
            !VERBOSE_ONLY_MESSAGES
                .iter()
                .any(|start| line.starts_with(start))
        })
        .map(str::to_string)
        .collect()
}

fn another_identity(others: &[String]) -> CheckResult {
    CheckResult::Inconclusive(format!(
        "another identity could have authenticated: {}",
        others.join(", ")
    ))
}

const PARTIAL_PUBLICKEY: &str = "Authenticated using \"publickey\" with partial success.";

/// Whether the first line of `log` that reports an authentication is
/// `PARTIAL_PUBLICKEY` and no disconnect message could have written it.
fn accepted_with_partial_success(log: &str) -> bool {
    !log.lines()
        .any(|line| line.starts_with("Received disconnect from "))
        && log
            .lines()
            .map(|line| line.trim_end_matches('\r'))
            .find(|line| {
                line.contains("Authenticated to ") || line.starts_with("Authenticated using \"")
            })
            .is_some_and(|line| line == PARTIAL_PUBLICKEY)
}

fn authenticated_method(log: &str) -> Option<&str> {
    let line = log
        .lines()
        .find(|line| line.contains("Authenticated to "))?;
    let after = &line[line.find("Authenticated to ")?..];
    let quoted = &after[after.find(" using \"")? + " using \"".len()..];
    quoted.split_once('"').map(|(method, _)| method)
}

fn failure_line(text: &str) -> Option<String> {
    trimmed_lines(text)
        .rfind(|line| {
            NO_AUTHENTICATION_FAILURES
                .iter()
                .any(|fragment| line.contains(fragment))
        })
        .map(str::to_string)
}

fn last_line(text: &str) -> Option<String> {
    trimmed_lines(text)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

fn trimmed_lines(text: &str) -> impl DoubleEndedIterator<Item = &str> {
    text.lines().map(|line| line.trim_end_matches('\r').trim())
}

/// The suffix of certificate key types.
const CERTIFICATE_SUFFIX: &[u8] = b"-cert-v01@openssh.com";

/// Whether the first key entry of validated input is a certificate, whose type
/// ends in `-cert-v01@openssh.com`.
///
/// `text` has passed `key_input::prepare`, so its first key entry line is
/// `keytype base64` or `options keytype base64`, where options may quote spaces
/// with double quotes. The base64 field cannot end in the suffix, so either of
/// the first two fields ending in it is the key type.
pub fn is_certificate(text: &[u8]) -> bool {
    let is_separator = |b: &u8| *b == b' ' || *b == b'\t';
    let Some(entry) = key_lines(text).into_iter().next().map(<[u8]>::trim_ascii) else {
        return false;
    };
    let first_end = first_field_end(entry);
    let rest = &entry[first_end..];
    let rest = &rest[rest
        .iter()
        .position(|b| !is_separator(b))
        .unwrap_or(rest.len())..];
    let second = &rest[..rest.iter().position(is_separator).unwrap_or(rest.len())];
    entry[..first_end].ends_with(CERTIFICATE_SUFFIX) || second.ends_with(CERTIFICATE_SUFFIX)
}

fn first_field_end(entry: &[u8]) -> usize {
    let mut in_quotes = false;
    let mut i = 0;
    while i < entry.len() {
        match entry[i] {
            b'\\' if in_quotes && entry.get(i + 1) == Some(&b'"') => i += 1,
            b'"' => in_quotes = !in_quotes,
            b' ' | b'\t' if !in_quotes => return i,
            _ => {}
        }
        i += 1;
    }
    entry.len()
}

/// Whether the first line of `ssh -V` output names a tested client version.
pub fn is_tested_client(version_output: &str) -> bool {
    let first = version_output.lines().next().unwrap_or("");
    TESTED_CLIENTS.iter().any(|tested| {
        first
            .strip_prefix(tested)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([',', ' ', '\r', '.']))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELECTED: &str = "C:/keys/sel";

    fn exists_except_own_certificate(p: &Path) -> bool {
        p != Path::new("C:/keys/sel-cert.pub") && p != Path::new("C:/keys/sel-cert")
    }

    fn textually_equal(a: &Path, b: &Path) -> bool {
        a == b
    }

    fn exists_none(_: &Path) -> bool {
        false
    }

    fn failure(result: CheckResult) -> Option<String> {
        match result {
            CheckResult::NotAttempted { failure, .. } => Some(failure),
            _ => None,
        }
    }

    fn messages(result: CheckResult) -> Vec<String> {
        match result {
            CheckResult::NotAttempted { messages, .. } => messages,
            other => panic!("not NotAttempted: {other:?}"),
        }
    }

    #[test]
    fn c01_only_the_selected_identity() {
        let config = format!("user u\nidentityfile {SELECTED}\nidentitiesonly yes\n");
        assert!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_none,
                &textually_equal
            )
            .is_empty()
        );
    }

    #[test]
    fn c02_a_configured_identity_that_exists_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\nidentityfile ~/.ssh/extra_key\n");
        let seen = std::cell::RefCell::new(Vec::new());
        let exists = |p: &Path| {
            seen.borrow_mut().push(p.to_path_buf());
            exists_except_own_certificate(p)
        };
        let others = other_candidates_matching(
            &config,
            SELECTED,
            Path::new("/h"),
            &exists,
            &textually_equal,
        );
        assert_eq!(others, vec!["identity file ~/.ssh/extra_key".to_string()]);
        assert!(
            seen.borrow()
                .contains(&Path::new("/h").join(".ssh/extra_key"))
        );
    }

    #[test]
    fn c03_a_configured_identity_that_is_absent_is_not_a_candidate() {
        let config = format!("identityfile {SELECTED}\nidentityfile ~/.ssh/extra_key\n");
        let exists = |p: &Path| {
            !p.ends_with("extra_key")
                && !p.ends_with("extra_key.pub")
                && exists_except_own_certificate(p)
        };
        assert!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists,
                &textually_equal
            )
            .is_empty()
        );
    }

    #[test]
    fn c04_a_configured_certificate_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\ncertificatefile ~/.ssh/c-cert.pub\n");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_except_own_certificate,
                &textually_equal
            ),
            vec!["certificate ~/.ssh/c-cert.pub".to_string()]
        );
    }

    #[test]
    fn c05_the_selected_key_s_own_certificate_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\n");
        let exists = |p: &Path| p == Path::new("C:/keys/sel-cert.pub");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists,
                &textually_equal
            ),
            vec!["certificate C:/keys/sel-cert.pub".to_string()]
        );
    }

    #[test]
    fn c06_a_pkcs11_provider_is_a_candidate_unless_none() {
        let with = format!("identityfile {SELECTED}\npkcs11provider /lib/p11.so\n");
        let without = format!("identityfile {SELECTED}\npkcs11provider none\n");
        assert_eq!(
            other_candidates_matching(
                &with,
                SELECTED,
                Path::new("/h"),
                &exists_none,
                &textually_equal
            ),
            vec!["PKCS#11 provider /lib/p11.so".to_string()]
        );
        assert!(
            other_candidates_matching(
                &without,
                SELECTED,
                Path::new("/h"),
                &exists_none,
                &textually_equal
            )
            .is_empty()
        );
    }

    #[test]
    fn c07_keys_are_matched_case_insensitively_and_values_keep_spaces() {
        let config = format!("identityfile {SELECTED}\nIdentityFile ~/my keys/k\n");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_except_own_certificate,
                &textually_equal
            ),
            vec!["identity file ~/my keys/k".to_string()]
        );
    }

    #[test]
    fn k01_success_with_no_other_candidate_is_installed() {
        let stderr = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".\r\n";
        assert_eq!(classify(Some(0), stderr, "", &[]), CheckResult::Installed);
    }

    #[test]
    fn k01b_success_without_an_authenticated_line_is_failed() {
        assert_eq!(
            classify(Some(0), "", "", &[]),
            CheckResult::Failed("ssh exited without recording an authentication".to_string())
        );
    }

    #[test]
    fn k02_success_with_another_candidate_is_inconclusive() {
        let others = vec!["identity file ~/.ssh/extra_key".to_string()];
        assert!(matches!(
            classify(Some(0), "", "", &others),
            CheckResult::Inconclusive(reason) if reason.contains("~/.ssh/extra_key")
        ));
    }

    #[test]
    fn k03_permission_denied_is_not_installed_even_with_other_candidates() {
        let stderr = "user@host: Permission denied (publickey).\r\n";
        assert_eq!(
            classify(Some(255), stderr, "", &[]),
            CheckResult::NotInstalled
        );
        let others = vec!["identity file x".to_string()];
        assert_eq!(
            classify(Some(255), stderr, "", &others),
            CheckResult::NotInstalled
        );
    }

    #[test]
    fn k04_host_key_failure_stops_the_run() {
        let stderr = "Host key verification failed.\r\n";
        assert_eq!(
            classify(Some(255), stderr, "", &[]),
            CheckResult::NotAttempted {
                failure: "Host key verification failed.".to_string(),
                messages: vec!["Host key verification failed.\r".to_string()],
            }
        );
    }

    #[test]
    fn k05_connection_failure_stops_the_run() {
        let stderr = "ssh: connect to host 127.0.0.1 port 1: Connection refused\r\n";
        assert!(matches!(
            classify(Some(255), stderr, "", &[]),
            CheckResult::NotAttempted { .. }
        ));
    }

    #[test]
    fn k06_failure_after_authentication_is_inconclusive() {
        assert!(matches!(
            classify(Some(1), "", "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k07_killed_probe_stops_the_run() {
        assert!(matches!(
            classify(None, "", "", &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k08_failed_message_is_the_last_line_naming_the_failure() {
        let stderr = "debug noise\r\nssh: Could not resolve hostname nowhere: No such host is known.\r\n\r\n";
        assert_eq!(
            failure(classify(Some(255), stderr, "", &[])).as_deref(),
            Some("ssh: Could not resolve hostname nowhere: No such host is known.")
        );
    }

    #[test]
    fn k09_success_by_another_method_is_inconclusive_naming_it() {
        let stderr = "Authenticated to h ([127.0.0.1]:22) using \"none\".";
        assert!(matches!(
            classify(Some(0), stderr, "", &[]),
            CheckResult::Inconclusive(reason) if reason.contains("none")
        ));
    }

    #[test]
    fn k10_changed_host_key_stops_the_run() {
        let stderr = "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@";
        assert!(matches!(
            classify(Some(255), stderr, "", &[]),
            CheckResult::NotAttempted { .. }
        ));
    }

    #[test]
    fn k11_connect_permission_denied_is_a_connection_failure() {
        let stderr = "ssh: connect to host h port 22: Permission denied";
        assert!(matches!(
            classify(Some(255), stderr, "", &[]),
            CheckResult::NotAttempted { .. }
        ));
    }

    #[test]
    fn k12_unrecognised_status_255_is_inconclusive_with_the_last_line() {
        assert!(matches!(
            classify(Some(255), "something unexpected", "", &[]),
            CheckResult::Inconclusive(reason) if reason.contains("something unexpected")
        ));
    }

    #[test]
    fn k13_publickey_outside_the_authenticated_line_is_not_installed() {
        let stderr = "Welcome. Last login using \"publickey\".
Authenticated to h ([10.0.0.1]:22) using \"none\".
";
        assert!(matches!(
            classify(Some(0), stderr, "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k14_publickey_in_a_banner_without_an_authenticated_line_is_failed() {
        let stderr = "Welcome. Last login using \"publickey\".
";
        assert!(matches!(
            classify(Some(0), "", stderr, &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k15_every_recognised_no_authentication_failure_stops_the_run() {
        let fragments = [
            "Host key verification failed",
            "REMOTE HOST IDENTIFICATION HAS CHANGED",
            "Connection refused",
            "Connection timed out",
            "Operation timed out",
            "Could not resolve hostname",
            "No route to host",
            "Network is unreachable",
            "Connection closed by",
            "Connection reset",
            "kex_exchange_identification",
            "connect to host",
        ];
        for fragment in fragments {
            let stderr = format!(
                "debug noise
ssh: {fragment}
"
            );
            assert_eq!(
                failure(classify(Some(255), &stderr, "", &[])),
                Some(format!("ssh: {fragment}")),
                "{fragment}"
            );
        }
    }

    #[test]
    fn k16_a_forged_authenticated_line_on_stderr_is_not_evidence() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"none\".
";
        let stderr = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        assert!(matches!(
            classify(Some(0), log, stderr, &[]),
            CheckResult::Inconclusive(reason) if reason.contains("none")
        ));
    }

    #[test]
    fn k17_publickey_on_stderr_with_an_empty_log_is_failed() {
        let stderr = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        assert!(matches!(
            classify(Some(0), "", stderr, &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k18_permission_denied_on_stderr_is_not_installed() {
        let stderr = "u@h: Permission denied (publickey).
";
        assert_eq!(
            classify(Some(255), "", stderr, &[]),
            CheckResult::NotInstalled
        );
    }

    #[test]
    fn k19_a_failure_on_stderr_stops_the_run_naming_its_line() {
        let log = "Transferred: sent 1, received 2 bytes
";
        let stderr = "Host key verification failed.
";
        assert_eq!(
            failure(classify(Some(255), log, stderr, &[])).as_deref(),
            Some("Host key verification failed.")
        );
    }

    #[test]
    fn k20_the_failure_line_is_named_even_when_other_lines_follow() {
        let log = "ssh: connect to host h port 22: Connection refused
";
        let stderr = "Goodbye.
";
        assert_eq!(
            failure(classify(Some(255), log, stderr, &[])).as_deref(),
            Some("ssh: connect to host h port 22: Connection refused")
        );
    }

    #[test]
    fn k26_ssh_s_messages_are_relayed_in_order_with_their_carriage_returns() {
        let log = "@@@@\r\nIT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\r\n\
                   The fingerprint for the ED25519 key sent by the remote host is\n\
                   SHA256:abc.\r\nHost key verification failed.\r\n";
        assert_eq!(
            messages(classify(Some(255), log, "", &[])),
            [
                "@@@@\r",
                "IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\r",
                "The fingerprint for the ED25519 key sent by the remote host is",
                "SHA256:abc.\r",
                "Host key verification failed.\r",
            ]
        );
    }

    #[test]
    fn k27_lines_only_log_level_verbose_writes_are_not_relayed() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".\r\n\
                   Authenticated to h (via proxy) using \"publickey\".\r\n\
                   Authenticated using \"publickey\" with partial success.\r\n\
                   Transferred: sent 3644, received 4004 bytes, in 0.1 seconds\r\n\
                   Bytes per second: sent 65149.9, received 71586.2\r\n\
                   Killed by signal 15.\r\n\
                   Canonicalized DNS aliased hostname \"a\" => \"b\"\r\n\
                   kex_exchange_identification: Connection closed by remote host\r\n\
                   kex_exchange_identification: banner line contains invalid characters\r\n\
                   kex_exchange_identification: banner line too long\r\n\
                   hostkeys_foreach_file: invalid marker at /k:1\r\n\
                   hostkeys_foreach_file: truncated line at /k:2\r\n\
                   Connection closed by 127.0.0.1 port 22\r\n";
        assert_eq!(
            messages(classify(Some(255), log, "", &[])),
            ["Connection closed by 127.0.0.1 port 22\r"]
        );
    }

    #[test]
    fn k28_stderr_lines_are_relayed_after_the_log_lines() {
        let log = "ssh: connect to host h port 22: Connection refused\r\n";
        let stderr = "Goodbye.\n";
        assert_eq!(
            messages(classify(Some(255), log, stderr, &[])),
            [
                "ssh: connect to host h port 22: Connection refused\r",
                "Goodbye."
            ]
        );
    }

    #[test]
    fn k21_publickey_in_the_log_is_installed_at_any_status_but_255() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        for status in [1, 127] {
            assert_eq!(
                classify(Some(status), log, "", &[]),
                CheckResult::Installed,
                "{status}"
            );
        }
        assert_ne!(classify(Some(255), log, "", &[]), CheckResult::Installed);
    }

    #[test]
    fn k23_a_publickey_line_planted_by_a_disconnect_message_is_not_installed() {
        let log = "Received disconnect from 127.0.0.1 port 22:11: bye
Authenticated to h ([127.0.0.1]:22) using \"publickey\".
Disconnected from 127.0.0.1 port 22
";
        assert!(matches!(
            classify(Some(255), log, "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k24_only_the_first_authenticated_line_counts() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"none\".
Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        assert!(matches!(
            classify(Some(1), log, "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k25_a_first_authenticated_line_without_a_method_hides_later_ones() {
        let log = "Authenticated to h ([127.0.0.1]:22)
Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        assert!(matches!(
            classify(Some(1), log, "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    const PARTIAL: &str = "Authenticated using \"publickey\" with partial success.\r\n\
                           u@h: Permission denied (password).\r\n";

    #[test]
    fn k29_publickey_with_partial_success_is_installed_without_other_candidates() {
        assert_eq!(
            classify(Some(255), PARTIAL, "", &[]),
            CheckResult::Installed
        );
    }

    #[test]
    fn k30_publickey_with_partial_success_and_another_candidate_is_inconclusive() {
        let others = vec!["identity file x".to_string()];
        assert!(matches!(
            classify(Some(255), PARTIAL, "", &others),
            CheckResult::Inconclusive(reason) if reason.contains("identity file x")
        ));
    }

    #[test]
    fn k31_partial_success_planted_by_a_disconnect_message_is_not_installed() {
        let log = "Received disconnect from 127.0.0.1 port 22:11: bye\r\n\
                   Authenticated using \"publickey\" with partial success.\r\n\
                   Disconnected from 127.0.0.1 port 22\r\n";
        assert_ne!(classify(Some(255), log, "", &[]), CheckResult::Installed);
    }

    #[test]
    fn k32_partial_success_of_another_method_is_not_installed() {
        let log = "Authenticated using \"password\" with partial success.\r\n\
                   u@h: Permission denied (publickey).\r\n";
        assert_eq!(classify(Some(255), log, "", &[]), CheckResult::NotInstalled);
    }

    #[test]
    fn k33_partial_success_after_another_authenticated_line_is_not_installed() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"none\".\r\n\
                   Authenticated using \"publickey\" with partial success.\r\n";
        assert_ne!(classify(Some(1), log, "", &[]), CheckResult::Installed);
    }

    #[test]
    fn k22_publickey_with_another_candidate_and_a_nonzero_exit_is_not_installed() {
        let log = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".
";
        let others = vec!["identity file x".to_string()];
        assert!(matches!(
            classify(Some(1), log, "", &others),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn c08_an_identity_that_is_the_same_file_as_the_selected_one_is_not_a_candidate() {
        let config = format!("identityfile {SELECTED}\nidentityfile ~/.ssh/same\n");
        let same_file = |a: &Path, b: &Path| {
            a == b || (a == Path::new("/h/.ssh/same") && b == Path::new(SELECTED))
        };
        assert!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_except_own_certificate,
                &same_file
            )
            .is_empty()
        );
    }

    #[test]
    fn c09_an_identity_with_a_percent_token_is_always_a_candidate() {
        let config = format!("identityfile {SELECTED}\nidentityfile %d/.ssh/work\n");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_none,
                &textually_equal
            ),
            vec!["identity file %d/.ssh/work".to_string()]
        );
    }

    #[test]
    fn c10_a_certificate_with_an_environment_reference_is_always_a_candidate() {
        let config = format!("identityfile {SELECTED}\ncertificatefile ${{HOME}}/.ssh/c\n");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_none,
                &textually_equal
            ),
            vec!["certificate ${HOME}/.ssh/c".to_string()]
        );
    }

    #[test]
    fn c11_the_selected_key_s_own_certificate_without_pub_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\n");
        let exists = |p: &Path| p == Path::new("C:/keys/sel-cert");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists,
                &textually_equal
            ),
            vec!["certificate C:/keys/sel-cert".to_string()]
        );
    }

    #[test]
    fn c12_an_identity_whose_public_half_alone_exists_is_a_candidate() {
        let config = format!(
            "identityfile {SELECTED}
identityfile ~/.ssh/agent_key
"
        );
        let exists = |p: &Path| p == Path::new("/h").join(".ssh/agent_key.pub");
        assert_eq!(
            other_candidates_matching(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists,
                &textually_equal
            ),
            vec!["identity file ~/.ssh/agent_key".to_string()]
        );
    }

    #[test]
    fn v01_tested_clients() {
        assert!(is_tested_client(
            "OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2
"
        ));
        assert!(is_tested_client(
            "OpenSSH_9.6p1 Ubuntu-3ubuntu13.14, OpenSSL 3.0.13 30 Jan 2024
"
        ));
    }

    #[test]
    fn v02_untested_clients() {
        assert!(!is_tested_client("OpenSSH_9.6p10, OpenSSL"));
        assert!(!is_tested_client(""));
    }

    #[test]
    fn v03_git_for_windows_client_is_tested() {
        assert!(is_tested_client(
            "OpenSSH_10.0p2, OpenSSL 3.2.4 11 Feb 2025
"
        ));
    }

    #[test]
    fn v04_unpatched_openssh_9_6p1_is_not_tested() {
        assert!(!is_tested_client(
            "OpenSSH_9.6p1, OpenSSL 3.0.13 30 Jan 2024"
        ));
    }

    #[test]
    fn v05_ubuntu_client_without_a_point_release_is_tested() {
        assert!(is_tested_client(
            "OpenSSH_9.6p1 Ubuntu-3ubuntu13, OpenSSL 3.0.13 30 Jan 2024"
        ));
    }

    #[test]
    fn v06_other_versions_of_a_tested_name_are_not_tested() {
        assert!(!is_tested_client("OpenSSH_10.0p20, OpenSSL"));
        assert!(!is_tested_client("OpenSSH_for_Windows_9.5p1, LibreSSL"));
        assert!(!is_tested_client(
            "OpenSSH_9.6p1 Ubuntu-3ubuntu130, OpenSSL"
        ));
    }

    #[test]
    fn v07_a_tested_name_is_matched_whole() {
        assert!(is_tested_client("OpenSSH_10.0p2"));
        assert!(is_tested_client("OpenSSH_10.0p2 x"));
        assert!(!is_tested_client("OpenSSH_10.0p20"));
    }

    #[test]
    fn e01_a_plain_key_is_not_a_certificate() {
        assert!(!is_certificate(b"# laptop\nssh-ed25519 AAAA me\n"));
    }

    #[test]
    fn e02_a_certificate_type_is_a_certificate() {
        assert!(is_certificate(
            b"# laptop\n\nssh-ed25519-cert-v01@openssh.com AAAA me\n"
        ));
    }

    #[test]
    fn e03_a_certificate_after_options_is_a_certificate() {
        assert!(is_certificate(
            b"command=\"echo a b\",restrict ecdsa-sha2-nistp256-cert-v01@openssh.com AAAA\n"
        ));
    }

    #[test]
    fn e04_the_suffix_inside_quoted_options_is_not_a_certificate() {
        assert!(!is_certificate(
            b"command=\"echo x ssh-ed25519-cert-v01@openssh.com\" ssh-ed25519 AAAA\n"
        ));
    }

    #[test]
    fn e05_a_cr_only_line_before_a_certificate_is_blank() {
        assert!(is_certificate(
            b"\r\n# c\r\nssh-ed25519-cert-v01@openssh.com AAAA\r\n"
        ));
    }
}
