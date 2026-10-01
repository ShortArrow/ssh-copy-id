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
/// certificate files are not offered by `ssh` and are not counted.
pub fn other_candidates(
    config: &str,
    selected: &str,
    home: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut others = Vec::new();
    for line in config.lines() {
        let line = line.trim_end_matches('\r');
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "identityfile" if value != selected && exists(&expand(home, value)) => {
                others.push(format!("identity file {value}"));
            }
            "certificatefile" if exists(&expand(home, value)) => {
                others.push(format!("certificate {value}"));
            }
            "pkcs11provider" if value != "none" => {
                others.push(format!("PKCS#11 provider {value}"));
            }
            _ => {}
        }
    }
    let own_certificate = format!("{selected}-cert.pub");
    if exists(&expand(home, &own_certificate)) {
        others.push(format!("certificate {own_certificate}"));
    }
    others
}

fn expand(home: &Path, value: &str) -> PathBuf {
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    }
}

/// Classifies a probe from its exit status and stderr, given the other candidates.
pub fn classify(exit: Option<i32>, stderr: &str, others: &[String]) -> CheckResult {
    match exit {
        Some(0) if others.is_empty() => CheckResult::Installed,
        Some(0) => CheckResult::Inconclusive(format!(
            "another identity could have authenticated: {}",
            others.join(", ")
        )),
        Some(255) if stderr.contains("Permission denied") => CheckResult::NotInstalled,
        Some(255) => CheckResult::Failed(last_line(stderr)),
        Some(code) => CheckResult::Inconclusive(format!(
            "the session failed after authentication with exit status {code}"
        )),
        None => CheckResult::Failed("ssh was terminated before it reported a result".to_string()),
    }
}

fn last_line(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .rfind(|line| !line.is_empty())
        .unwrap_or("ssh exited with status 255")
        .to_string()
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
        p != Path::new("C:/keys/sel-cert.pub")
    }

    fn exists_none(_: &Path) -> bool {
        false
    }

    #[test]
    fn c01_only_the_selected_identity() {
        let config = format!("user u\nidentityfile {SELECTED}\nidentitiesonly yes\n");
        assert!(other_candidates(&config, SELECTED, Path::new("/h"), &exists_none).is_empty());
    }

    #[test]
    fn c02_a_configured_identity_that_exists_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\nidentityfile ~/.ssh/extra_key\n");
        let seen = std::cell::RefCell::new(Vec::new());
        let exists = |p: &Path| {
            seen.borrow_mut().push(p.to_path_buf());
            exists_except_own_certificate(p)
        };
        let others = other_candidates(&config, SELECTED, Path::new("/h"), &exists);
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
        assert!(other_candidates(&config, SELECTED, Path::new("/h"), &exists).is_empty());
    }

    #[test]
    fn c04_a_configured_certificate_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\ncertificatefile ~/.ssh/c-cert.pub\n");
        assert_eq!(
            other_candidates(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_except_own_certificate
            ),
            vec!["certificate ~/.ssh/c-cert.pub".to_string()]
        );
    }

    #[test]
    fn c05_the_selected_key_s_own_certificate_is_a_candidate() {
        let config = format!("identityfile {SELECTED}\n");
        let exists = |p: &Path| p == Path::new("C:/keys/sel-cert.pub");
        assert_eq!(
            other_candidates(&config, SELECTED, Path::new("/h"), &exists),
            vec!["certificate C:/keys/sel-cert.pub".to_string()]
        );
    }

    #[test]
    fn c06_a_pkcs11_provider_is_a_candidate_unless_none() {
        let with = format!("identityfile {SELECTED}\npkcs11provider /lib/p11.so\n");
        let without = format!("identityfile {SELECTED}\npkcs11provider none\n");
        assert_eq!(
            other_candidates(&with, SELECTED, Path::new("/h"), &exists_none),
            vec!["PKCS#11 provider /lib/p11.so".to_string()]
        );
        assert!(other_candidates(&without, SELECTED, Path::new("/h"), &exists_none).is_empty());
    }

    #[test]
    fn c07_keys_are_matched_case_insensitively_and_values_keep_spaces() {
        let config = format!("IdentityFile {SELECTED}\nidentityfile ~/my keys/k\n");
        assert_eq!(
            other_candidates(
                &config,
                SELECTED,
                Path::new("/h"),
                &exists_except_own_certificate
            ),
            vec!["identity file ~/my keys/k".to_string()]
        );
    }

    #[test]
    fn k01_success_with_no_other_candidate_is_installed() {
        assert_eq!(classify(Some(0), "", &[]), CheckResult::Installed);
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
