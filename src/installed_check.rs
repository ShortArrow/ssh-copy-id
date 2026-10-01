//! The installed-key check: which identities could answer the probe, and what the probe's result means.

use std::path::{Path, PathBuf};

/// The result of checking whether the selected key is already installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The selected key was the only candidate and it authenticated.
    Installed,
    /// The server rejected public-key authentication.
    NotInstalled,
    /// The probe cannot tell; the key is installed with this reason as a warning.
    Inconclusive(String),
    /// The probe could not reach a verdict about authentication, such as a host
    /// key mismatch or a connection failure; the run stops with this message.
    Failed(String),
}

/// Client version strings, as `ssh -V` prints them, that the fixtures have tested.
pub const TESTED_CLIENTS: [&str; 2] = ["OpenSSH_for_Windows_9.5p2", "OpenSSH_9.6p1"];

/// Lists the identities other than `selected` that `ssh` could offer, read from `ssh -G` output.
///
/// `selected` is the `-i` argument exactly as passed to `ssh`. `home` expands a
/// leading `~/`. `exists` reports whether a file is present; absent identity and
/// certificate files are not offered by `ssh` and are not counted. `same_file`
/// reports whether two paths name the same file; an `identityfile` value that
/// equals `selected` textually or names the same file is not a candidate.
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
    let mut others = Vec::new();
    for line in config.lines() {
        let line = line.trim_end_matches('\r');
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "identityfile" if !is_selected(value) && is_present(value) => {
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

/// Stderr fragments of a status-255 probe that mean the host key was rejected
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

/// Classifies a probe from its exit status and stderr, given the other candidates.
///
/// Exit 0 is `Installed` only when no other candidate exists and the
/// `Authenticated to ... using "<method>"` line of stderr names `publickey`
/// (the probe runs with `LogLevel=VERBOSE`); `using "publickey"` elsewhere, such
/// as in a banner, is not evidence. A server accepting another method, such as
/// `none`, lets the probe succeed without the key. Exit 255 is `NotInstalled` on `Permission denied (`,
/// `Failed` on a recognised host key or connection failure, and `Inconclusive`
/// otherwise.
pub fn classify(exit: Option<i32>, stderr: &str, others: &[String]) -> CheckResult {
    match exit {
        Some(0) if !others.is_empty() => CheckResult::Inconclusive(format!(
            "another identity could have authenticated: {}",
            others.join(", ")
        )),
        Some(0) if authenticated_method(stderr) == Some("publickey") => CheckResult::Installed,
        Some(0) => CheckResult::Inconclusive(without_selected_key(stderr)),
        Some(255) if stderr.contains("Permission denied (") => CheckResult::NotInstalled,
        Some(255) => match last_line(stderr) {
            Some(line) if contains_any(stderr, &NO_AUTHENTICATION_FAILURES) => {
                CheckResult::Failed(line)
            }
            Some(line) => CheckResult::Inconclusive(format!("ssh exited with status 255: {line}")),
            None => CheckResult::Inconclusive("ssh exited with status 255".to_string()),
        },
        Some(code) => CheckResult::Inconclusive(format!(
            "the session failed after authentication with exit status {code}"
        )),
        None => CheckResult::Failed("ssh was terminated before it reported a result".to_string()),
    }
}

fn without_selected_key(stderr: &str) -> String {
    match authenticated_method(stderr) {
        Some(method) => {
            format!("authentication succeeded without the selected key, using \"{method}\"")
        }
        None => "authentication succeeded without the selected key".to_string(),
    }
}

fn authenticated_method(stderr: &str) -> Option<&str> {
    stderr.lines().find_map(|line| {
        let after = &line[line.find("Authenticated to ")?..];
        let quoted = &after[after.find(" using \"")? + " using \"".len()..];
        quoted.split_once('"').map(|(method, _)| method)
    })
}

fn contains_any(text: &str, fragments: &[&str]) -> bool {
    fragments.iter().any(|fragment| text.contains(fragment))
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// Whether the first line of `ssh -V` output names a tested client version.
pub fn is_tested_client(version_output: &str) -> bool {
    let first = version_output.lines().next().unwrap_or("");
    TESTED_CLIENTS.iter().any(|tested| {
        first
            .strip_prefix(tested)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([',', ' ', '\r']))
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
        let exists = |p: &Path| !p.ends_with("extra_key") && exists_except_own_certificate(p);
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
        assert_eq!(classify(Some(0), stderr, &[]), CheckResult::Installed);
    }

    #[test]
    fn k01b_success_without_publickey_evidence_is_inconclusive() {
        assert!(matches!(
            classify(Some(0), "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k02_success_with_another_candidate_is_inconclusive() {
        let others = vec!["identity file ~/.ssh/extra_key".to_string()];
        assert!(matches!(
            classify(Some(0), "", &others),
            CheckResult::Inconclusive(reason) if reason.contains("~/.ssh/extra_key")
        ));
    }

    #[test]
    fn k03_permission_denied_is_not_installed_even_with_other_candidates() {
        let stderr = "user@host: Permission denied (publickey).\r\n";
        assert_eq!(classify(Some(255), stderr, &[]), CheckResult::NotInstalled);
        let others = vec!["identity file x".to_string()];
        assert_eq!(
            classify(Some(255), stderr, &others),
            CheckResult::NotInstalled
        );
    }

    #[test]
    fn k04_host_key_failure_stops_the_run() {
        let stderr = "Host key verification failed.\r\n";
        assert_eq!(
            classify(Some(255), stderr, &[]),
            CheckResult::Failed("Host key verification failed.".to_string())
        );
    }

    #[test]
    fn k05_connection_failure_stops_the_run() {
        let stderr = "ssh: connect to host 127.0.0.1 port 1: Connection refused\r\n";
        assert!(matches!(
            classify(Some(255), stderr, &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k06_failure_after_authentication_is_inconclusive() {
        assert!(matches!(
            classify(Some(1), "", &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k07_killed_probe_stops_the_run() {
        assert!(matches!(classify(None, "", &[]), CheckResult::Failed(_)));
    }

    #[test]
    fn k08_failed_message_is_the_last_nonempty_stderr_line() {
        let stderr = "debug noise\r\nssh: Could not resolve hostname nowhere: No such host is known.\r\n\r\n";
        assert_eq!(
            classify(Some(255), stderr, &[]),
            CheckResult::Failed(
                "ssh: Could not resolve hostname nowhere: No such host is known.".to_string()
            )
        );
    }

    #[test]
    fn k09_success_by_another_method_is_inconclusive_naming_it() {
        let stderr = "Authenticated to h ([127.0.0.1]:22) using \"none\".";
        assert!(matches!(
            classify(Some(0), stderr, &[]),
            CheckResult::Inconclusive(reason) if reason.contains("none")
        ));
    }

    #[test]
    fn k10_changed_host_key_stops_the_run() {
        let stderr = "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@";
        assert!(matches!(
            classify(Some(255), stderr, &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k11_connect_permission_denied_is_a_connection_failure() {
        let stderr = "ssh: connect to host h port 22: Permission denied";
        assert!(matches!(
            classify(Some(255), stderr, &[]),
            CheckResult::Failed(_)
        ));
    }

    #[test]
    fn k12_unrecognised_status_255_is_inconclusive_with_the_last_line() {
        assert!(matches!(
            classify(Some(255), "something unexpected", &[]),
            CheckResult::Inconclusive(reason) if reason.contains("something unexpected")
        ));
    }

    #[test]
    fn k13_publickey_outside_the_authenticated_line_is_not_installed() {
        let stderr = "Welcome. Last login using \"publickey\".
Authenticated to h ([10.0.0.1]:22) using \"none\".
";
        assert!(matches!(
            classify(Some(0), stderr, &[]),
            CheckResult::Inconclusive(_)
        ));
    }

    #[test]
    fn k14_publickey_in_a_banner_without_an_authenticated_line_is_inconclusive() {
        let stderr = "Welcome. Last login using \"publickey\".
";
        assert!(matches!(
            classify(Some(0), stderr, &[]),
            CheckResult::Inconclusive(_)
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
                classify(Some(255), &stderr, &[]),
                CheckResult::Failed(format!("ssh: {fragment}")),
                "{fragment}"
            );
        }
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
    fn v01_tested_clients() {
        assert!(is_tested_client(
            "OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2\r\n"
        ));
        assert!(is_tested_client(
            "OpenSSH_9.6p1 Ubuntu-3ubuntu13.19, OpenSSL 3.0.13 30 Jan 2024\n"
        ));
    }

    #[test]
    fn v02_untested_clients() {
        assert!(!is_tested_client(
            "OpenSSH_10.0p2, OpenSSL 3.2.4 11 Feb 2025"
        ));
        assert!(!is_tested_client("OpenSSH_9.6p10, OpenSSL"));
        assert!(!is_tested_client(""));
    }
}
