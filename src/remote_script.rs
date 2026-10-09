//! The installation command sent to Unix-like destinations.

use crate::result_line::encode_path;

/// Quotes `text` as one POSIX shell word: single quotes, with each `'` written as `'\''`.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A `-t` path, which reaches the installation script as one line of its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPath(String);

impl TargetPath {
    /// The path as given, or `None` when it holds LF and so cannot be one line.
    /// Every other character, CR included, is data.
    pub fn new(path: &str) -> Option<TargetPath> {
        (!path.contains('\n')).then(|| TargetPath(path.to_string()))
    }

    /// The path as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Builds the remote command that appends the key lines read from stdin to a target file.
///
/// `None` targets `.ssh/authorized_keys` with upstream's special cases: OpenWrt as
/// root uses `/etc/dropbear/authorized_keys` and Haiku uses
/// `config/settings/ssh/authorized_keys`. `Some(target)` uses the target as
/// given on every destination. A relative target is relative to the home
/// directory; missing parent directories are created under `umask 077`, and
/// existing ones keep their modes.
///
/// The command holds no part of the target: the script reads it from stdin, as
/// [`install_input`] writes it, so quotes, `!` and CR in it are data whatever
/// the login shell. The command depends only on whether a target is given.
/// Every command that takes the target or its directory as an operand gets it
/// after `--` or as an `of=` value, so a path starting with `-` is a file name.
///
/// The command runs `exec sh -c` with the script quoted as one word, so the
/// destination's login shell only has to run `exec`. The command is one line
/// without `!`, as csh and tcsh require.
///
/// Every line of stdin after the target's lines is appended, comment lines and
/// blank lines included; only the other lines are keys. As in `key_input`, a
/// comment line's first character other than space and tab is `#`, and a blank
/// line holds only spaces and tabs. A newline is added first when the target is
/// non-empty and does not end with one.
///
/// The script prints one result line per key and a summary line last, in the
/// format `result_line::parse_report` reads; `path=` names the target actually
/// used. When the home directory cannot be entered, a target that exists and is
/// not a regular file, or a non-empty target that cannot be read, nothing is
/// written: every key is reported failed.
///
/// Lines are written in groups: the comment and blank lines since the last
/// written key plus the next key, the added newline belonging to the first
/// group and trailing comment and blank lines forming a group of their own.
/// When a write fails, the target is truncated back to its size before the
/// group, or removed when it did not exist before the group; no later line is
/// written, and the group's key and every later key are reported failed. The
/// truncation happens only when the bytes after that size are a prefix of the
/// bytes this run tried to write in the group, the added newline included;
/// anything else, such as a line another writer appended, is left in place.
/// When the truncation is skipped or cannot be confirmed, the group's key is
/// reported `uncertain` and the summary is `uncertain`, whatever was added
/// before.
///
/// Each write runs in a subshell whose output is the target. bash 3.2, the
/// `/bin/sh` of macOS, keeps the unwritten part of a failed `printf` buffered
/// and writes it to the next output; inside the subshell that output is the
/// target again, so the remainder cannot reach the size check or the result
/// lines.
pub fn install_command(target: Option<&TargetPath>) -> String {
    let selection = match target {
        None => DEFAULT_TARGET_SELECTION,
        Some(_) => EXPLICIT_TARGET_SELECTION,
    };
    let script = format!("{} {}", one_line(selection), one_line(INSTALL_SCRIPT));
    format!("exec sh -c {}", sh_quote(&script))
}

/// The stdin of [`install_command`]: with a target, the path and then its
/// `result_line::encode_path` form, each as one line, before `keys`; without
/// one, `keys` alone.
pub fn install_input(target: Option<&TargetPath>, keys: &[u8]) -> Vec<u8> {
    let mut input = Vec::new();
    if let Some(TargetPath(path)) = target {
        input.extend_from_slice(path.as_bytes());
        input.push(b'\n');
        input.extend_from_slice(encode_path(path.as_bytes()).as_bytes());
        input.push(b'\n');
    }
    input.extend_from_slice(keys);
    input
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

const EXPLICIT_TARGET_SELECTION: &str = r##"IFS= read -r f;
IFS= read -r p;
"##;

const INSTALL_SCRIPT: &str = r##"umask 077;
failed=0;
t=$(printf '\t');
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
is_comment_or_blank() {
    expr "x$1" : "x[ $t]*#" >/dev/null || expr "x$1" : "x[ $t]*\$" >/dev/null;
};
hex() {
    od -v -An -tx1 | tr -d ' \t\n';
};
append() {
    if [ "$wrote" -eq 0 ] && [ -s "$f" ]; then
        last=$(tail -c 1 -- "$f" | hex);
        case $last in 0a) ;; *) tried=${tried}0a; ( printf '\n' ) >> "$f" || return 1 ;; esac;
    fi;
    tried=$tried$(printf '%s\n' "$1" | hex);
    ( printf '%s\n' "$1" ) >> "$f";
};
roll_back() {
    if [ -z "$1" ]; then rm -f -- "$f"; else dd if=/dev/null of="$f" bs=1 seek="$1" 2>/dev/null; fi;
    [ "$(size_of)" = "$1" ];
};
restore_group() {
    now=$(size_of);
    if [ -z "$now" ]; then [ -z "$base" ]; return; fi;
    [ "$now" -ge "${base:-0}" ] || return 1;
    since=$(tail -c +$((${base:-0} + 1)) -- "$f" | hex);
    case $tried in "$since"*) roll_back "$base" ;; *) return 1 ;; esac;
};
n=0;
added=0;
wrote=0;
uncertain=0;
tried=;
base=;
after_failure=failed;
[ "$failed" -ne 0 ] || base=$(size_of);
while IFS= read -r line || [ -n "$line" ]; do
    if is_comment_or_blank "$line"; then key=0; else key=1; n=$((n + 1)); fi;
    if [ "$failed" -eq 0 ]; then
        if append "$line"; then
            [ "$key" -eq 0 ] || { base=$(size_of); tried=; };
        else
            failed=1;
            restore_group || { after_failure=uncertain; uncertain=1; };
        fi;
        wrote=1;
    fi;
    if [ "$key" -eq 1 ]; then
        if [ "$failed" -eq 0 ]; then
            status=added;
            added=$((added + 1));
        else
            status=$after_failure;
            after_failure=failed;
        fi;
        printf 'ssh-copy-id: key=%s result=%s path=%s\n' "$n" "$status" "$p";
    fi;
done;
if [ "$added" -gt 0 ] && command -v restorecon >/dev/null 2>&1; then
    restorecon -F -- "$d" "$f" >/dev/null 2>&1;
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

        fn run_after(&self, setup: &str, target: Option<&str>, keys: &str) -> (Report, String) {
            let target = target.map(|path| TargetPath::new(path).expect("a one-line path"));
            let stdin = install_input(target.as_ref(), keys.as_bytes());
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(format!("{setup} {}", install_command(target.as_ref())))
                .env("HOME", &self.0)
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("sh must be on PATH for these tests");
            use std::io::Write;
            child.stdin.take().unwrap().write_all(&stdin).unwrap();
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
        let target = TargetPath::new(".ssh/odd 'name' 名前!\r").unwrap();
        for command in [install_command(None), install_command(Some(&target))] {
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
        let command = install_command(Some(&TargetPath::new(".ssh/x").unwrap()));
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

    #[cfg(unix)]
    fn uncertain(index: usize, path: &[u8]) -> KeyResult {
        KeyResult {
            index,
            status: KeyStatus::Uncertain,
            path: path.to_vec(),
        }
    }

    #[cfg(unix)]
    fn executable(home: &Home, relative: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(home.file(relative).parent().unwrap()).unwrap();
        fs::write(home.file(relative), body).unwrap();
        fs::set_permissions(home.file(relative), fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn with_bin_on_path(home: &Home, limit: &str) -> String {
        format!(
            "{limit} PATH={}:$PATH;",
            sh_quote(&home.file("bin").to_string_lossy())
        )
    }

    #[test]
    fn s23_indented_comment_and_space_only_line_are_not_keys() {
        let home = Home::new();
        let input = format!("  # indented\n\t\n{KEY_A}\n");
        let (report, _) = home.run(None, &input);
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(
            fs::read_to_string(home.file(".ssh/authorized_keys")).unwrap(),
            input
        );
    }

    #[cfg(unix)]
    #[test]
    fn s24_short_key_after_a_failed_comment_is_not_written() {
        let home = Home::new();
        let long_comment = format!("# {}", "c".repeat(1200));
        let (report, _) = home.run_after(
            "trap '' XFSZ; ulimit -f 1;",
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
        assert!(!home.file(".ssh/authorized_keys").exists());
    }

    #[cfg(unix)]
    #[test]
    fn s25_newline_added_for_a_failed_key_is_removed() {
        let home = Home::new();
        let (_, long_key) = existing_and_long_line();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), "existing").unwrap();
        let (report, _) = home.run_after(SMALL_FILE_LIMIT, None, &format!("{long_key}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(
            fs::read(home.file(".ssh/authorized_keys")).unwrap(),
            b"existing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn s26_comment_before_a_failed_key_is_rolled_back_with_it() {
        let home = Home::new();
        let (existing, long_key) = existing_and_long_line();
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(home.file(".ssh/authorized_keys"), &existing).unwrap();
        let (report, _) = home.run_after(SMALL_FILE_LIMIT, None, &format!("# short\n{long_key}\n"));
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
    fn s27_file_created_for_a_comment_and_failed_key_is_removed() {
        let home = Home::new();
        let (_, long_key) = existing_and_long_line();
        let (report, _) = home.run_after(SMALL_FILE_LIMIT, None, &format!("# short\n{long_key}\n"));
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
    fn append_once_before(home: &Home, tool: &str, first_arg: &str, printf_args: &str) {
        let target = home.file(".ssh/authorized_keys");
        executable(
            home,
            &format!("bin/{tool}"),
            &format!(
                "#!/bin/sh\ncase $1 in {first_arg}) [ -e \"$0.done\" ] || {{ ulimit -S -f unlimited; printf {printf_args} >> {}; : > \"$0.done\"; }} ;; esac\nPATH=${{PATH#*:}}\nexport PATH\nexec {tool} \"$@\"\n",
                sh_quote(&target.to_string_lossy())
            ),
        );
    }

    #[cfg(unix)]
    fn line_of(text: &str) -> String {
        format!("'%s\\n' {}", sh_quote(text))
    }

    #[cfg(unix)]
    #[test]
    fn s28_concurrent_append_is_not_truncated() {
        let home = Home::new();
        let other = "o".repeat(3000);
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, "e\n").unwrap();
        append_once_before(&home, "tail", "*", &line_of(&other));
        let setup = with_bin_on_path(&home, "trap '' XFSZ; ulimit -S -f 2;");
        let long_key = format!("{KEY_A} {}", "c".repeat(3000));
        let (report, _) = home.run_after(&setup, None, &format!("{long_key}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![uncertain(1, b".ssh/authorized_keys")],
            }
        );
        let after = fs::read_to_string(&target).unwrap();
        assert!(
            after.starts_with(&format!("e\n{other}\n")),
            "the other writer's line must survive: {after:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn s32_small_concurrent_append_before_a_partly_written_key_is_not_truncated() {
        let home = Home::new();
        let (_, long_key) = existing_and_long_line();
        let other = "o".repeat(10);
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, "e\n").unwrap();
        append_once_before(&home, "tail", "*", &line_of(&other));
        let setup = with_bin_on_path(&home, "trap '' XFSZ; ulimit -S -f 2;");
        let (report, _) = home.run_after(&setup, None, &format!("{long_key}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![uncertain(1, b".ssh/authorized_keys")],
            }
        );
        let content = fs::read_to_string(&target).unwrap();
        assert!(content.starts_with(&format!("e\n{other}\n")), "{content}");
    }

    #[cfg(unix)]
    #[test]
    fn s33_small_concurrent_append_before_a_partly_written_trailing_comment_is_not_truncated() {
        let home = Home::new();
        let long_comment = format!("# {}", "c".repeat(2000));
        let other = "o".repeat(10);
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        append_once_before(&home, "expr", "x#*", &line_of(&other));
        let setup = with_bin_on_path(&home, "trap '' XFSZ; ulimit -S -f 2;");
        let (report, _) = home.run_after(&setup, None, &format!("{KEY_A}\n{long_comment}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        let content = fs::read_to_string(&target).unwrap();
        assert!(
            content.starts_with(&format!("{KEY_A}\n{other}\n")),
            "{content}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn s34_concurrent_nul_byte_before_a_partly_written_key_is_not_truncated() {
        let home = Home::new();
        let (_, long_key) = existing_and_long_line();
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, "e\n").unwrap();
        append_once_before(&home, "tail", "*", r"'\000'");
        let setup = with_bin_on_path(&home, "trap '' XFSZ; ulimit -S -f 2;");
        let (report, _) = home.run_after(&setup, None, &format!("{long_key}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![uncertain(1, b".ssh/authorized_keys")],
            }
        );
        assert!(fs::read(&target).unwrap().starts_with(b"e\n\0"));
    }

    #[cfg(unix)]
    #[test]
    fn s36_partly_written_trailing_comment_is_rolled_back_and_the_key_kept() {
        let home = Home::new();
        let long_comment = format!("# {}", "c".repeat(2000));
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, "e\n").unwrap();
        let (report, stdout) = home.run_after(
            SMALL_FILE_LIMIT,
            None,
            &format!("{KEY_A}\n{long_comment}\n"),
        );
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Partial,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        assert!(
            stdout.ends_with("ssh-copy-id: result=partial added=1\n"),
            "{stdout}"
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            format!("e\n{KEY_A}\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn s37_target_shrunk_below_its_size_before_the_group_is_not_extended() {
        let home = Home::new();
        let (existing, long_key) = existing_and_long_line();
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, &existing).unwrap();
        executable(
            &home,
            "bin/wc",
            &format!(
                "#!/bin/sh\nif [ -e \"$0.first\" ]; then [ -e \"$0.done\" ] || {{ printf 'short\\n' > {}; : > \"$0.done\"; }}; else : > \"$0.first\"; fi\nPATH=${{PATH#*:}}\nexport PATH\nexec wc \"$@\"\n",
                sh_quote(&target.to_string_lossy())
            ),
        );
        let setup = with_bin_on_path(&home, SMALL_FILE_LIMIT);
        let (report, _) = home.run_after(&setup, None, &format!("{long_key}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![uncertain(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(fs::read(&target).unwrap(), b"short\n");
    }

    #[cfg(unix)]
    #[test]
    fn s35_home_that_cannot_be_entered_gets_nothing_written() {
        let home = Home::new();
        let setup = format!(
            "cd {}; HOME={}; export HOME;",
            sh_quote(&home.0.to_string_lossy()),
            sh_quote(&home.file("missing").to_string_lossy())
        );
        let (report, _) = home.run_after(&setup, None, &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        assert!(!home.file(".ssh").exists());
        assert!(!home.file("missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn s29_unconfirmed_rollback_of_a_trailing_comment_is_uncertain() {
        let home = Home::new();
        let long_comment = format!("# {}", "c".repeat(2000));
        executable(&home, "bin/dd", "#!/bin/sh\nexit 0\n");
        let setup = with_bin_on_path(&home, SMALL_FILE_LIMIT);
        let (report, _) = home.run_after(&setup, None, &format!("{KEY_A}\n{long_comment}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn s30_empty_write_only_file_is_written() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        let target = home.file(".ssh/authorized_keys");
        fs::create_dir_all(home.file(".ssh")).unwrap();
        fs::write(&target, "").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o200)).unwrap();
        let (report, _) = home.run(None, &format!("{KEY_A}\n"));
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, b".ssh/authorized_keys")],
            }
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), format!("{KEY_A}\n"));
    }

    #[cfg(unix)]
    #[test]
    fn s31_rollback_compares_bytes_not_characters() {
        let home = Home::new();
        let wide_key = format!("{KEY_A} {}", "あ".repeat(1000));
        let (report, _) = home.run_after(
            "LC_ALL=C.UTF-8; export LC_ALL; trap '' XFSZ; ulimit -f 2;",
            None,
            &format!("{wide_key}\n"),
        );
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b".ssh/authorized_keys")],
            }
        );
        assert!(!home.file(".ssh/authorized_keys").exists());
    }

    #[test]
    fn s38_target_path_with_shell_syntax_is_data() {
        let home = Home::new();
        let target = "keys dir/it's $(touch pwned) `touch pwned` ! 名前";
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
        assert!(!home.file("pwned").exists());
        assert!(!home.file("keys dir/pwned").exists());
    }

    #[cfg(unix)]
    #[test]
    fn s39_target_path_with_double_quote_backslash_cr_and_blanks_is_data() {
        let home = Home::new();
        let target = "  odd \"q\" back\\slash \r\tname  ";
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
    fn s40_command_holds_no_part_of_the_target_path() {
        let marked = TargetPath::new("keys/Xq7-it's 名前").unwrap();
        let other = TargetPath::new("other").unwrap();
        let command = install_command(Some(&marked));
        assert_eq!(command, install_command(Some(&other)));
        for part in ["Xq7", "名前", &encode_path("名前".as_bytes())] {
            assert!(!command.contains(part), "{part} in {command}");
        }
    }

    #[test]
    fn s41_input_holds_the_path_and_its_encoded_form_before_the_keys() {
        let target = TargetPath::new("a b%").unwrap();
        assert_eq!(
            install_input(Some(&target), b"k\n"),
            b"a b%\na%20b%25\nk\n".to_vec()
        );
        assert_eq!(install_input(None, b"k\n"), b"k\n".to_vec());
    }

    #[test]
    fn s42_a_target_path_must_be_one_line() {
        assert_eq!(TargetPath::new("a\nb"), None);
        assert_eq!(TargetPath::new("a\n"), None);
        assert_eq!(
            TargetPath::new("a\rb").map(|t| t.as_str().to_string()),
            Some("a\rb".to_string())
        );
    }

    #[cfg(unix)]
    fn mode_of(path: PathBuf) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn s43_missing_parents_of_a_relative_target_are_created_private() {
        let home = Home::new();
        let (report, _) = home.run(Some("keys/new/authorized"), &format!("{KEY_A}\n"));
        assert_eq!(report.outcome, Outcome::Installed);
        assert_eq!(mode_of(home.file("keys")), 0o700);
        assert_eq!(mode_of(home.file("keys/new")), 0o700);
        assert_eq!(mode_of(home.file("keys/new/authorized")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn s44_existing_shared_parent_of_an_absolute_target_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        fs::create_dir_all(home.file("shared")).unwrap();
        fs::set_permissions(home.file("shared"), fs::Permissions::from_mode(0o755)).unwrap();
        let target = home.file("shared/keys").to_string_lossy().into_owned();
        let (report, _) = home.run(Some(&target), &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Installed,
                keys: vec![added(1, target.as_bytes())],
            }
        );
        assert_eq!(mode_of(home.file("shared")), 0o755);
        assert_eq!(mode_of(home.file("shared/keys")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn s45_unwritable_parent_is_reported_and_left_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        fs::create_dir_all(home.file("locked")).unwrap();
        fs::set_permissions(home.file("locked"), fs::Permissions::from_mode(0o555)).unwrap();
        let (report, _) = home.run(Some("locked/keys"), &format!("{KEY_A}\n"));
        assert_eq!(
            report,
            Report {
                outcome: Outcome::Unchanged,
                keys: vec![failed(1, b"locked/keys")],
            }
        );
        assert!(!home.file("locked/keys").exists());
        assert_eq!(mode_of(home.file("locked")), 0o555);
    }

    #[cfg(unix)]
    #[test]
    fn s46_haiku_target_applies_only_without_an_explicit_target() {
        let home = Home::new();
        executable(&home, "bin/uname", "#!/bin/sh\necho Haiku\n");
        let setup = with_bin_on_path(&home, "");
        let (default, _) = home.run_after(&setup, None, &format!("{KEY_A}\n"));
        assert_eq!(
            default.keys,
            vec![added(1, b"config/settings/ssh/authorized_keys")]
        );
        let (explicit, _) = home.run_after(&setup, Some(".ssh/x"), &format!("{KEY_B}\n"));
        assert_eq!(explicit.keys, vec![added(1, b".ssh/x")]);
        assert_eq!(
            fs::read_to_string(home.file(".ssh/x")).unwrap(),
            format!("{KEY_B}\n")
        );
        assert_eq!(
            fs::read_to_string(home.file("config/settings/ssh/authorized_keys")).unwrap(),
            format!("{KEY_A}\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn s47_target_paths_starting_with_a_dash_are_operands() {
        let home = Home::new();
        executable(
            &home,
            "bin/restorecon",
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HOME/restorecon.args\"\n",
        );
        let setup = with_bin_on_path(&home, "");
        let (report, _) = home.run_after(&setup, Some("-R"), &format!("{KEY_A}\n"));
        assert_eq!(report.keys, vec![added(1, b"-R")]);
        assert_eq!(
            fs::read_to_string(home.file("-R")).unwrap(),
            format!("{KEY_A}\n")
        );
        let (report, _) = home.run_after(&setup, Some("-v/-n"), &format!("{KEY_B}\n"));
        assert_eq!(report.keys, vec![added(1, b"-v/-n")]);
        assert_eq!(
            fs::read_to_string(home.file("-v/-n")).unwrap(),
            format!("{KEY_B}\n")
        );
        assert_eq!(
            fs::read_to_string(home.file("restorecon.args")).unwrap(),
            "-F\n--\n.\n-R\n-F\n--\n-v\n-v/-n\n"
        );
    }
}
