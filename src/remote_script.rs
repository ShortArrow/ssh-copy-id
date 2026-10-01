//! The installation command sent to Unix-like destinations.

use crate::result_line::encode_path;

/// Quotes `text` as one POSIX shell word: single quotes, with each `'` written as `'\''`.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Builds the remote command that appends the key lines read from stdin to a target file.
///
/// `None` targets `.ssh/authorized_keys` with upstream's special cases: OpenWrt as
/// root uses `/etc/dropbear/authorized_keys` and Haiku uses
/// `config/settings/ssh/authorized_keys`. `Some(target)` uses `target` as given.
/// A relative target is relative to the home directory.
///
/// The command runs `exec sh -c` with the script quoted as one word, so the
/// destination's login shell only has to run `exec`. The command is one line
/// without `!`, as csh and tcsh require, provided `target` contains neither a
/// line break nor `!`.
///
/// Every line of stdin is appended, `#` comment lines and blank lines included;
/// only the other lines are keys. A newline is added first when the target is
/// non-empty and does not end with one.
///
/// The script prints one result line per key and a summary line last, in the
/// format `result_line::parse_report` reads; `path=` names the target actually
/// used. A target that exists and is not a regular file, or a non-empty target
/// that cannot be read, is not written: every key is reported failed.
///
/// When a line's write fails, the target is truncated back to its size before
/// that write, or removed when it did not exist; that line and every later key
/// are reported failed. When the rollback cannot be confirmed, that key is
/// reported `uncertain` and the summary is `uncertain`.
pub fn install_command(target: Option<&str>) -> String {
    let script = format!("{} {}", target_selection(target), one_line(INSTALL_SCRIPT));
    format!("exec sh -c {}", sh_quote(&script))
}

fn target_selection(target: Option<&str>) -> String {
    match target {
        None => one_line(DEFAULT_TARGET_SELECTION),
        Some(path) => format!(
            "f={}; p={};",
            sh_quote(path),
            sh_quote(&encode_path(path.as_bytes()))
        ),
    }
}

fn one_line(script: &str) -> String {
    script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

const DEFAULT_TARGET_SELECTION: &str = r##"f=.ssh/authorized_keys;
if [ -f /etc/openwrt_release ] && { [ "$LOGNAME" = root ] || [ "$(id -u)" = 0 ]; }; then
    f=/etc/dropbear/authorized_keys;
fi;
if [ "$(uname -s)" = Haiku ]; then
    f=config/settings/ssh/authorized_keys;
fi;
p=$f;
"##;

const INSTALL_SCRIPT: &str = r##"umask 077;
failed=0;
cd || failed=1;
d=$(dirname -- "$f");
[ "$failed" -ne 0 ] || mkdir -p -- "$d" || failed=1;
if [ "$failed" -eq 0 ] && [ -s "$f" ]; then
    [ -r "$f" ] || failed=1;
fi;
if [ "$failed" -eq 0 ] && [ -e "$f" ]; then
    [ -f "$f" ] || failed=1;
fi;
size_of() {
    if [ -s "$f" ]; then wc -c < "$f" | tr -d ' '; elif [ -e "$f" ]; then echo 0; fi;
};
append() {
    if [ "$wrote" -eq 0 ] && [ -s "$f" ]; then
        last=$(tail -c 1 -- "$f" | od -An -tx1 | tr -d ' ');
        case $last in 0a) ;; *) printf '\n' >> "$f" || return 1 ;; esac;
    fi;
    printf '%s\n' "$1" >> "$f";
};
roll_back() {
    if [ -z "$1" ]; then rm -f -- "$f"; else dd if=/dev/null of="$f" bs=1 seek="$1" 2>/dev/null; fi;
    [ "$(size_of)" = "$1" ];
};
n=0;
added=0;
wrote=0;
uncertain=0;
while IFS= read -r line || [ -n "$line" ]; do
    case $line in ''|'#'*) key=0 ;; *) key=1; n=$((n + 1)) ;; esac;
    status=failed;
    if [ "$failed" -eq 0 ]; then
        size=$(size_of);
        if append "$line"; then
            status=added;
        else
            failed=1;
            roll_back "$size" || { status=uncertain; uncertain=1; };
        fi;
        wrote=1;
    fi;
    if [ "$key" -eq 1 ]; then
        case $status in added) added=$((added + 1)) ;; esac;
        printf 'ssh-copy-id: key=%s result=%s path=%s\n' "$n" "$status" "$p";
    fi;
done;
if [ "$added" -gt 0 ] && command -v restorecon >/dev/null 2>&1; then
    restorecon -F "$d" "$f" >/dev/null 2>&1;
fi;
if [ "$uncertain" -ne 0 ]; then
    r=uncertain;
elif [ "$failed" -ne 0 ] && [ "$added" -gt 0 ]; then
    r=partial;
elif [ "$added" -gt 0 ]; then
    r=installed;
else
    r=unchanged;
fi;
printf 'ssh-copy-id: result=%s added=%s\n' "$r" "$added";
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_line::{KeyResult, KeyStatus, Outcome, Report, parse_report};
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== a@host";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB== b@host";
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Home(PathBuf);

    impl Home {
        fn new() -> Home {
            let path = std::env::temp_dir().join(format!(
                "ssh-copy-id-script-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Home(path)
        }

        fn file(&self, relative: &str) -> PathBuf {
            self.0.join(relative)
        }

        fn run(&self, target: Option<&str>, stdin: &str) -> (Report, String) {
            self.run_after("", target, stdin)
        }

        fn run_after(&self, setup: &str, target: Option<&str>, stdin: &str) -> (Report, String) {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(format!("{setup} {}", install_command(target)))
                .env("HOME", &self.0)
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("sh must be on PATH for these tests");
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(stdin.as_bytes())
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait().unwrap().is_none() {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    panic!("the script did not finish within 20 s");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let output = child.wait_with_output().unwrap();
            (
                parse_report(&output.stdout),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for entry in walk(&self.0) {
                    let _ = fs::set_permissions(&entry, fs::Permissions::from_mode(0o700));
                }
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn walk(root: &std::path::Path) -> Vec<PathBuf> {
        let mut found = vec![root.to_path_buf()];
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                found.extend(walk(&entry.path()));
            }
        }
        found
    }

    fn added(index: usize, path: &[u8]) -> KeyResult {
        KeyResult {
            index,
            status: KeyStatus::Added,
            path: path.to_vec(),
        }
    }

    #[cfg(unix)]
    fn failed(index: usize, path: &[u8]) -> KeyResult {
        KeyResult {
            index,
            status: KeyStatus::Failed,
            path: path.to_vec(),
        }
    }

    #[test]
    fn s01_sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn s02_missing_directory_and_file_are_created() {
        let home = Home::new();
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("{KEY_A}\n")
        );
    }

    #[test]
    fn s03_empty_file_gets_the_key() {
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "").unwrap();
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(report.outcome, Outcome::Installed);
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("{KEY_A}\n")
        );
    }

    #[test]
    fn s04_missing_final_newline_is_added_before_the_key() {
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "existing").unwrap();
        home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("existing\n{KEY_A}\n")
        );
    }

    #[test]
    fn s05_existing_final_newline_is_kept() {
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "existing\n").unwrap();
        home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("existing\n{KEY_A}\n")
        );
    }

    #[test]
    fn s06_two_keys_are_reported_in_order() {
        let home = Home::new();
        let (report, _) = home.run(None, &format!("{KEY_A}\n{KEY_B}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![
                    added(1, b".ssh/authorized_keys"),
                    added(2, b".ssh/authorized_keys"),
                ],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("{KEY_A}\n{KEY_B}\n")
        );
    }

    #[test]
    fn s07_path_with_quote_and_space_is_data() {
        let home = Home::new();
        let target = ".ssh/odd 'name'";
        let (report, _) = home.run(Some(target), &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, target.as_bytes())],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(target)).unwrap(),
            format!("{KEY_A}\n")
        );
    }

    #[test]
    fn s08_leading_spaces_and_quotes_in_the_key_line_are_kept() {
        let home = Home::new();
        let line = format!("  command=\"echo 'hi' $HOME\" {KEY_A}");
        home.run(None, &format!("{line}\n"));
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("{line}\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn s09_new_directory_and_file_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        home.run(None, &format!("{KEY_A}\n"));
        let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(home.file(".ssh")), 0o700);
        assert_eq!(mode(home.file(".ssh/authorized_keys")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn s10_unwritable_file_is_left_unchanged_and_reported() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "existing\n").unwrap();
        fs::set_permissions(
            home.file(".ssh/authorized_keys"),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![KeyResult {
                    index: 1,
                    status: KeyStatus::Failed,
                    path: b".ssh/authorized_keys".to_vec(),
                }],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            "existing\n"
        );
    }

    #[test]
    fn s11_command_has_no_newline_or_exclamation_mark_for_csh() {
        for command in [
            install_command(None),
            install_command(Some(".ssh/odd 'name' 名前")),
        ] {
            assert!(!command.contains('\n'), "{command}");
            assert!(!command.contains('\r'), "{command}");
            assert!(!command.contains('!'), "{command}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn s12_unreadable_nonempty_file_is_not_written() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "existing").unwrap();
        fs::set_permissions(
            home.file(".ssh/authorized_keys"),
            fs::Permissions::from_mode(0o200),
        )
        .unwrap();
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![KeyResult {
                    index: 1,
                    status: KeyStatus::Failed,
                    path: b".ssh/authorized_keys".to_vec(),
                }],
            }
        );
        fs::set_permissions(
            home.file(".ssh/authorized_keys"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn s13_final_nul_byte_gets_a_newline_before_the_key() {
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), b"existing\0").unwrap();
        home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            fs::read(home.file(".ssh/authorized_keys")).unwrap(),
            format!("existing\0\n{KEY_A}\n").into_bytes()
        );
    }

    #[test]
    fn s14_default_target_keeps_upstream_special_cases() {
        let command = install_command(None);
        for expected in [
            "/etc/openwrt_release",
            "/etc/dropbear/authorized_keys",
            "Haiku",
            "config/settings/ssh/authorized_keys",
        ] {
            assert!(command.contains(expected), "{expected} in {command}");
        }
    }

    #[test]
    fn s15_explicit_target_has_no_special_cases() {
        let command = install_command(Some(".ssh/x"));
        for unexpected in ["/etc/openwrt_release", "Haiku"] {
            assert!(!command.contains(unexpected), "{unexpected} in {command}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn s16_fifo_target_is_not_opened() {
        use std::os::unix::fs::FileTypeExt;
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        let made = Command::new("mkfifo")
            .arg(home.file(".ssh/authorized_keys"))
            .status()
            .unwrap();
        assert!(made.success());
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        let kind = fs::symlink_metadata(home.file(".ssh/authorized_keys"))
            .unwrap()
            .file_type();
        assert!(kind.is_fifo());
    }

    #[test]
    fn s17_comment_and_blank_lines_are_written_but_not_counted() {
        let home = Home::new();
        let (report, _) = home.run(None, &format!("# laptop\n\n{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("# laptop\n\n{KEY_A}\n")
        );
    }

    #[test]
    fn s18_missing_final_newline_is_added_before_a_leading_comment() {
        let home = Home::new();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "x").unwrap();
        home.run(None, &format!("# c\n{KEY_A}\n"));
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("x\n# c\n{KEY_A}\n")
        );
    }

    #[cfg(unix)]
    const SMALL_FILE_LIMIT: &str = "trap '' XFSZ; ulimit -f 2;";

    #[cfg(unix)]
    fn existing_and_long_line() -> (String, String) {
        (
            "e".repeat(999) + "\n",
            format!("{KEY_A} {}", "c".repeat(2000)),
        )
    }

    #[cfg(unix)]
    #[test]
    fn s19_partly_written_key_is_rolled_back() {
        let home = Home::new();
        let (existing, long_key) = existing_and_long_line();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), &existing).unwrap();
        let (report, _) = home.run_after(SMALL_FILE_LIMIT, None, &format!("{long_key}\n{KEY_B}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![
                    failed(1, b".ssh/authorized_keys"),
                    failed(2, b".ssh/authorized_keys"),
                ],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            existing
        );
    }

    #[cfg(unix)]
    #[test]
    fn s20_partly_written_comment_is_rolled_back_and_later_keys_fail() {
        let home = Home::new();
        let (existing, _) = existing_and_long_line();
        let long_comment = format!("# {}", "c".repeat(2000));
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), &existing).unwrap();
        let (report, _) = home.run_after(
            SMALL_FILE_LIMIT,
            None,
            &format!("{long_comment}\n{KEY_A}\n"),
        );
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            existing
        );
    }

    #[cfg(unix)]
    #[test]
    fn s21_file_created_by_a_failed_write_is_removed() {
        let home = Home::new();
        let (report, _) = home.run_after("trap '' XFSZ; ulimit -f 0;", None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        assert!(!home.file(".ssh/authorized_keys").exists());
    }

    #[cfg(unix)]
    #[test]
    fn s22_unconfirmed_rollback_is_uncertain() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        let (existing, long_key) = existing_and_long_line();
        fs::create_dir_all(home.file("bin")).unwrap();
        fs::write(home.file("bin/dd"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(home.file("bin/dd"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), &existing).unwrap();
        let setup = format!(
            "{SMALL_FILE_LIMIT} PATH={}:$PATH;",
            sh_quote(&home.file("bin").to_string_lossy())
        );
        let (report, _) = home.run_after(&setup, None, &format!("{long_key}\n{KEY_B}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![
                    KeyResult {
                        index: 1,
                        status: KeyStatus::Uncertain,
                        path: b".ssh/authorized_keys".to_vec(),
                    },
                    failed(2, b".ssh/authorized_keys"),
                ],
            }
        );
    }
}
