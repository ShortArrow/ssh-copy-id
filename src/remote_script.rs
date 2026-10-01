//! The installation command sent to Unix-like destinations.

use crate::result_line::encode_path;

/// Quotes `text` as one POSIX shell word: single quotes, with each `'` written as `'\''`.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Builds the remote command that appends the key lines read from stdin to `target`.
///
/// `target` is relative to the home directory unless absolute. The command runs
/// `exec sh -c` with the script quoted as one word, so the destination's login
/// shell only has to run `exec`. The script prints one result line per key and a
/// summary line last, in the format `result_line::parse_report` reads.
pub fn unix_install_command(target: &str) -> String {
    let script = INSTALL_SCRIPT
        .replace("@TARGET@", &sh_quote(target))
        .replace("@PATH@", &sh_quote(&encode_path(target.as_bytes())));
    format!("exec sh -c {}", sh_quote(&script))
}

const INSTALL_SCRIPT: &str = r##"umask 077
failed=0
cd || failed=1
f=@TARGET@
p=@PATH@
d=$(dirname -- "$f")
[ "$failed" -ne 0 ] || mkdir -p -- "$d" || failed=1
n=0
added=0
while IFS= read -r line || [ -n "$line" ]; do
    case $line in ''|'#'*) continue ;; esac
    n=$((n + 1))
    if [ "$failed" -eq 0 ] && [ "$n" -eq 1 ] && [ -s "$f" ] && [ -n "$(tail -c 1 -- "$f")" ]; then
        printf '\n' >> "$f" || failed=1
    fi
    if [ "$failed" -eq 0 ] && printf '%s\n' "$line" >> "$f"; then
        added=$((added + 1))
        printf 'ssh-copy-id: key=%s result=added path=%s\n' "$n" "$p"
    else
        failed=1
        printf 'ssh-copy-id: key=%s result=failed path=%s\n' "$n" "$p"
    fi
done
if [ "$added" -gt 0 ] && command -v restorecon >/dev/null 2>&1; then
    restorecon -F "$d" "$f" >/dev/null 2>&1
fi
if [ "$failed" -ne 0 ] && [ "$added" -gt 0 ]; then
    r=partial
elif [ "$added" -gt 0 ]; then
    r=installed
else
    r=unchanged
fi
printf 'ssh-copy-id: result=%s added=%s\n' "$r" "$added"
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_line::{KeyResult, KeyStatus, Outcome, Report, parse_report};
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

        fn run(&self, target: &str, stdin: &str) -> (Report, String) {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(unix_install_command(target))
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

    #[test]
    fn s01_sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn s02_missing_directory_and_file_are_created() {
        let home = Home::new();
        let (report, _) = home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
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
        let (report, _) = home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
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
        home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
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
        home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            format!("existing\n{KEY_A}\n")
        );
    }

    #[test]
    fn s06_two_keys_are_reported_in_order() {
        let home = Home::new();
        let (report, _) = home.run(".ssh/authorized_keys", &format!("{KEY_A}\n{KEY_B}\n"));
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
        let (report, _) = home.run(target, &format!("{KEY_A}\n"));
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
        home.run(".ssh/authorized_keys", &format!("{line}\n"));
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
        home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
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
        let (report, _) = home.run(".ssh/authorized_keys", &format!("{KEY_A}\n"));
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
}
