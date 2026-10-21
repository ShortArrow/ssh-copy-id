//! The installation command sent to Windows destinations.

/// Builds `powershell.exe -NoProfile -NonInteractive -EncodedCommand <base64>`,
/// whose script appends the key lines read from stdin to `.ssh\authorized_keys`
/// under the profile directory and prints the result lines that
/// `result_line::parse_report` reads, as `remote_script::install_command`'s
/// does.
///
/// The command holds only letters, digits and base64, so `cmd.exe /c`,
/// `powershell.exe -c` and `pwsh -c` pass it unchanged, and it fits in the
/// 8191 characters `cmd.exe` accepts.
///
/// The script reads stdin as bytes and appends every line byte for byte, a CR
/// at its end and comment and blank lines included, each followed by LF; only
/// the other lines are keys. Comment and blank lines, the newline added after
/// a last line without one, the groups of lines, the rollback of a failed
/// group and the reported results follow `remote_script::install_command`.
/// The rollback compares the bytes past the group's starting size with the
/// bytes the run tried to write; anything else is left and reported
/// `uncertain`. A missing directory or file is created with the ACL it
/// inherits.
///
/// An account whose token holds the Administrators group, SID `S-1-5-32-544`,
/// even for deny only, gets nothing written: the script says so on stderr and
/// reports every key failed with the path of `administrators_authorized_keys`
/// under `$env:ProgramData\ssh`. So does a target that is a directory, and a
/// non-empty target that cannot be read, with the default path. A `$HOME` that
/// is not a directory also gets nothing written, so that nothing goes to the
/// working directory; PowerShell creates a missing profile directory before the
/// script runs.
pub fn windows_install_command() -> String {
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encoded(&format!(
            "{ADMIN_CHECK}\n{INSTALL_FUNCTIONS}\n{INSTALL_MAIN}"
        ))
    )
}

/// Sets `$a` when the account's token holds the Administrators group.
const ADMIN_CHECK: &str = "$a=[bool]([Security.Principal.WindowsIdentity]::GetCurrent().Claims|?{$_.Value -eq 'S-1-5-32-544'})";

/// The functions [`INSTALL_MAIN`] calls, with the error handling of both. `_w`
/// appends bytes to the target, `_a` a line in its group, and `_r` rolls the
/// group back. Functions start with `_` because an alias, such as `r` for
/// `Invoke-History`, takes precedence over a function of the same name.
const INSTALL_FUNCTIONS: &str = r#"$ErrorActionPreference='Stop'
trap{[Console]::Error.WriteLine("ssh-copy-id: ERROR: the installation script stopped: $_");exit 1}
function _o($s){[Console]::Out.Write("ssh-copy-id: $s`n")}
function _e($s){-join([Text.Encoding]::UTF8.GetBytes($s)|%{if($_ -ge 33 -and $_ -le 126 -and $_ -ne 37 -and $_ -ne 61){[string][char]$_}else{'%{0:X2}' -f $_}})}
function _s{if([IO.File]::Exists($f)){(New-Object IO.FileInfo $f).Length}}
function _w([byte[]]$b){try{$s=[IO.File]::Open($f,'Append','Write','ReadWrite');try{$s.Write($b,0,$b.Length);$s.Flush()}finally{$s.Dispose()};1}catch{0}}
function _l{$s=New-Object IO.FileStream($f,'Open','Read','ReadWrite',1);try{$null=$s.Seek(-1,'End');$s.ReadByte()}finally{$s.Dispose()}}
function _a([byte[]]$l){try{if(!$w -and (_s) -gt 0 -and (_l) -ne 10){$t.Add(10);if(!(_w @(10))){return 0}};$t.AddRange($l);$t.Add(10);_w ([byte[]]($l+[byte[]]@(10)))}catch{0}}
function _r{try{$n=_s;if($n -eq $null){return $z -eq $null};$b=[long]$z;$k=New-Object byte[] ($n-$b);$s=New-Object IO.FileStream($f,'Open','Read','ReadWrite',1);try{$null=$s.Seek($b,'Begin');$q=0;while($q -lt $k.Length){$c=$s.Read($k,$q,$k.Length-$q);if(!$c){break};$q+=$c}}finally{$s.Dispose()};for($j=0;$j -lt $k.Length;$j++){if($k[$j] -ne $t[$j]){return $false}};if($z -eq $null){[IO.File]::Delete($f)}else{$s=[IO.File]::Open($f,'Open','Write','ReadWrite');try{$s.SetLength($z)}finally{$s.Dispose()}};(_s) -eq $z}catch{$false}}"#;

/// Appends stdin's lines and reports them, writing nothing when `$a` is set.
/// `$x` is set once nothing more may be written, `$z` holds the target's size
/// before the current group (`$null` when it did not exist) and `$t` the bytes
/// tried in the group.
const INSTALL_MAIN: &str = r#"$f=[IO.Path]::Combine($HOME,'.ssh','authorized_keys');$p='.ssh/authorized_keys';$x=0
if($a){$x=1;$p=_e([IO.Path]::Combine($env:ProgramData,'ssh','administrators_authorized_keys'));[Console]::Error.WriteLine('ssh-copy-id: ERROR: administrators_authorized_keys is not supported in this release; nothing was written')}
$m=New-Object IO.MemoryStream;[Console]::OpenStandardInput().CopyTo($m);$i=$m.ToArray()
if(!$x){try{if(![IO.Directory]::Exists($HOME)){throw 1};$null=[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($f));if((_s) -gt 0){[IO.File]::Open($f,'Open','Read','ReadWrite').Dispose()}}catch{$x=1}}
$n=0;$g=0;$w=0;$u=0;$h='failed';$t=New-Object Collections.Generic.List[byte];$z=$null;if(!$x){$z=_s}
$o=0;for($e=0;$e -le $i.Length;$e++){if($e -lt $i.Length -and $i[$e] -ne 10){continue};if($e -eq $o -and $e -eq $i.Length){break}
$l=[byte[]]@();if($e -gt $o){$l=[byte[]]$i[$o..($e-1)]};$o=$e+1
$v=$l.Length;if($v -and $l[$v-1] -eq 13){$v--};$c=0;while($c -lt $v -and ($l[$c] -eq 32 -or $l[$c] -eq 9)){$c++};$k=$c -lt $v -and $l[$c] -ne 35;if($k){$n++}
if(!$x){if(_a $l){if($k){$z=_s;$t.Clear()}}else{$x=1;if(!(_r)){$h='uncertain';$u=1}};$w=1}
if($k){if(!$x){$y='added';$g++}else{$y=$h;$h='failed'};_o "key=$n result=$y path=$p"}}
$r='unchanged';if($u){$r='uncertain'}elseif($x -and $g){$r='partial'}elseif($g){$r='installed'};_o "result=$r added=$g""#;

/// `script` as `-EncodedCommand` takes it: base64 of its UTF-16LE bytes.
fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// Base64 with padding, as RFC 4648 section 4 defines it.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(char::from(DIGITS[(bits >> (18 - 6 * i)) as usize & 63]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_line::{KeyResult, KeyStatus, Outcome, Report, parse_report};

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== a@here";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB== b@here";
    const DEFAULT_PATH: &[u8] = b".ssh/authorized_keys";
    const PREFIX: &str = "powershell.exe -NoProfile -NonInteractive -EncodedCommand ";

    fn key(index: usize, status: KeyStatus, path: &[u8]) -> KeyResult {
        KeyResult {
            index,
            status,
            path: path.to_vec(),
        }
    }

    fn decoded(text: &str) -> Vec<u8> {
        let value = |c: u8| match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
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
            let count = digits.len() * 6 / 8;
            bytes.extend((0..count).map(|i| (bits >> (16 - 8 * i)) as u8));
        }
        bytes
    }

    #[test]
    fn w01_base64_pads_as_rfc_4648_does() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(&[0xFB, 0xFF]), "+/8=");
    }

    #[test]
    fn w02_the_command_is_the_fixed_line_and_base64_alone() {
        let command = windows_install_command();
        let payload = command.strip_prefix(PREFIX).expect("the fixed prefix");
        assert!(
            payload
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
            "{payload}"
        );
    }

    #[test]
    fn w03_the_payload_is_the_utf16le_script() {
        let command = windows_install_command();
        let bytes = decoded(command.strip_prefix(PREFIX).unwrap());
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            script_with(ADMIN_CHECK, "")
        );
        assert!(
            !INSTALL_MAIN.is_empty() && !INSTALL_FUNCTIONS.is_empty() && !ADMIN_CHECK.is_empty()
        );
    }

    /// The script with `prelude` before it and `between` after its functions,
    /// where a definition replaces the script's own.
    fn script_with(prelude: &str, between: &str) -> String {
        let between = if between.is_empty() {
            String::new()
        } else {
            format!("{between}\n")
        };
        format!("{prelude}\n{INSTALL_FUNCTIONS}\n{between}{INSTALL_MAIN}")
    }

    #[test]
    fn w04_the_command_fits_in_a_cmd_exe_line() {
        let wrapped = format!("cmd.exe /c \"{}\"", windows_install_command());
        assert!(wrapped.len() <= 8191, "{} characters", wrapped.len());
    }

    #[test]
    fn w05_the_script_is_ascii() {
        assert!(INSTALL_MAIN.is_ascii() && INSTALL_FUNCTIONS.is_ascii() && ADMIN_CHECK.is_ascii());
    }

    #[cfg(windows)]
    mod run {
        use super::*;
        use std::fs;
        use std::path::PathBuf;
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);

        /// A temporary profile directory, removed when dropped.
        struct Profile(PathBuf);

        impl Profile {
            fn new() -> Profile {
                let path = std::env::temp_dir().join(format!(
                    "ssh-copy-id-windows-script-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::SeqCst)
                ));
                fs::create_dir_all(&path).unwrap();
                Profile(path)
            }

            fn file(&self, relative: &str) -> PathBuf {
                self.0.join(relative)
            }

            fn keys(&self) -> PathBuf {
                self.file(".ssh\\authorized_keys")
            }

            /// Runs the installation script with `$a` set to `admin` and the
            /// profile as `$HOME`, and returns its report, stdout and stderr.
            fn run_as(&self, admin: bool, stdin: &[u8]) -> (Report, String, String) {
                let flag = if admin { "$true" } else { "$false" };
                run_script(&script_with(&format!("$a={flag}"), ""), &self.0, stdin)
            }

            /// Runs the script as a standard user with `writer` in place of
            /// its `_w`.
            fn run_writing_with(&self, writer: &str, stdin: &[u8]) -> (Report, String) {
                let (report, stdout, _) =
                    run_script(&script_with("$a=$false", writer), &self.0, stdin);
                (report, stdout)
            }

            fn run(&self, stdin: &[u8]) -> (Report, String) {
                let (report, stdout, _) = self.run_as(false, stdin);
                (report, stdout)
            }
        }

        impl Drop for Profile {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn run_script(
            script: &str,
            profile: &std::path::Path,
            stdin: &[u8],
        ) -> (Report, String, String) {
            use std::io::Write;
            let mut child = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encoded(script))
                .env("USERPROFILE", profile)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("powershell.exe must be on PATH for these tests");
            child.stdin.take().unwrap().write_all(stdin).unwrap();
            let output = child.wait_with_output().unwrap();
            (
                parse_report(&output.stdout),
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        }

        fn installed(keys: Vec<KeyResult>) -> Report {
            Report {
                outcome: Outcome::Installed,
                keys,
            }
        }

        fn unchanged(keys: Vec<KeyResult>) -> Report {
            Report {
                outcome: Outcome::Unchanged,
                keys,
            }
        }

        #[test]
        fn w06_missing_directory_and_file_are_created() {
            let profile = Profile::new();
            let (report, stdout) = profile.run(format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, DEFAULT_PATH)]),
                "{stdout}"
            );
            assert_eq!(
                fs::read(profile.keys()).unwrap(),
                format!("{KEY_A}\n").as_bytes()
            );
        }

        #[test]
        fn w07_missing_final_newline_is_added_before_the_key() {
            let profile = Profile::new();
            fs::create_dir_all(profile.file(".ssh")).unwrap();
            fs::write(profile.keys(), "existing").unwrap();
            let (report, _) = profile.run(format!("{KEY_A}\n").as_bytes());
            assert_eq!(report.outcome, Outcome::Installed);
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("existing\n{KEY_A}\n")
            );
        }

        #[test]
        fn w08_existing_final_newline_and_crlf_are_kept() {
            let profile = Profile::new();
            fs::create_dir_all(profile.file(".ssh")).unwrap();
            fs::write(profile.keys(), "existing\r\n").unwrap();
            let (report, _) = profile.run(format!("{KEY_A}\r\n{KEY_B}").as_bytes());
            assert_eq!(
                report,
                installed(vec![
                    key(1, KeyStatus::Added, DEFAULT_PATH),
                    key(2, KeyStatus::Added, DEFAULT_PATH),
                ])
            );
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("existing\r\n{KEY_A}\r\n{KEY_B}\n")
            );
        }

        #[test]
        fn w09_comment_and_blank_lines_are_written_but_not_counted() {
            let profile = Profile::new();
            let input =
                format!("# 日本語のコメント\n \t\n\r\n  # indented\r\n{KEY_A} 鍵\n# last\n");
            let (report, _) = profile.run(input.as_bytes());
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, DEFAULT_PATH)])
            );
            assert_eq!(fs::read(profile.keys()).unwrap(), input.as_bytes());
        }

        #[test]
        fn w10_an_administrator_gets_nothing_written() {
            let profile = Profile::new();
            let (report, _, stderr) =
                profile.run_as(true, format!("{KEY_A}\n{KEY_B}\n").as_bytes());
            let shared = crate::result_line::encode_path(
                format!(
                    "{}\\ssh\\administrators_authorized_keys",
                    std::env::var("ProgramData").unwrap()
                )
                .as_bytes(),
            );
            assert_eq!(
                report,
                unchanged(vec![
                    key(1, KeyStatus::Failed, shared.as_bytes()),
                    key(2, KeyStatus::Failed, shared.as_bytes()),
                ])
            );
            assert!(
                stderr.contains("administrators_authorized_keys is not supported"),
                "{stderr}"
            );
            assert!(!profile.file(".ssh").exists());
        }

        #[test]
        fn w11_a_directory_as_the_target_gets_nothing_written() {
            let profile = Profile::new();
            fs::create_dir_all(profile.keys()).unwrap();
            let (report, _) = profile.run(format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, DEFAULT_PATH)])
            );
        }

        #[test]
        fn w13_a_target_that_cannot_be_read_gets_nothing_written() {
            use std::os::windows::fs::OpenOptionsExt;
            let profile = Profile::new();
            fs::create_dir_all(profile.file(".ssh")).unwrap();
            fs::write(profile.keys(), "existing\n").unwrap();
            let _held = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(profile.keys())
                .unwrap();
            let (report, _) = profile.run(format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, DEFAULT_PATH)])
            );
            drop(_held);
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "existing\n");
        }

        #[test]
        fn w14_the_admin_check_agrees_with_whoami() {
            let groups = Command::new("whoami.exe").arg("/groups").output().unwrap();
            let expected = String::from_utf8_lossy(&groups.stdout).contains("S-1-5-32-544");
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encoded(&format!(
                    "{ADMIN_CHECK}\n[Console]::Out.Write([int]$a)"
                )))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                if expected { "1" } else { "0" }
            );
        }

        /// A `_w` that writes the first call's bytes and fails every later call.
        const FAILS_AFTER_ONE_WRITE: &str = "$script:q=0;function _w([byte[]]$b){$script:q++;\
             if($script:q -gt 1){return 0};$s=[IO.File]::Open($f,'Append','Write','ReadWrite');\
             $s.Write($b,0,$b.Length);$s.Dispose();1}";

        #[test]
        fn w20_a_file_created_by_a_failed_group_is_removed() {
            let profile = Profile::new();
            let (report, stdout) = profile
                .run_writing_with(FAILS_AFTER_ONE_WRITE, format!("# c\n{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, DEFAULT_PATH)]),
                "{stdout}"
            );
            assert!(!profile.keys().exists());
        }

        #[test]
        fn w21_a_half_written_key_is_truncated() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let (report, _) = profile.run_writing_with(
                "function _w([byte[]]$b){$s=[IO.File]::Open($f,'Append','Write','ReadWrite');\
                 $s.Write($b,0,[int]($b.Length/2));$s.Dispose();0}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, DEFAULT_PATH)])
            );
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "e\n");
        }

        #[test]
        fn w22_another_writer_s_line_is_kept_and_the_outcome_is_uncertain() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let (report, _) = profile.run_writing_with(
                "function _w([byte[]]$b){$o=[byte[]](111,10);\
                 $s=[IO.File]::Open($f,'Append','Write','ReadWrite');\
                 $s.Write($o,0,2);$s.Dispose();0}",
                format!("{KEY_A}\n{KEY_B}\n").as_bytes(),
            );
            assert_eq!(
                report,
                Report {
                    outcome: Outcome::Uncertain,
                    keys: vec![
                        key(1, KeyStatus::Uncertain, DEFAULT_PATH),
                        key(2, KeyStatus::Failed, DEFAULT_PATH),
                    ],
                }
            );
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "e\no\n");
        }

        #[test]
        fn w23_another_writer_s_line_after_the_group_s_bytes_is_kept() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let (report, _) = profile.run_writing_with(
                "function _w([byte[]]$b){$b+=[byte[]](111,10);\
                 $s=[IO.File]::Open($f,'Append','Write','ReadWrite');\
                 $s.Write($b,0,$b.Length);$s.Dispose();0}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(
                report,
                Report {
                    outcome: Outcome::Uncertain,
                    keys: vec![key(1, KeyStatus::Uncertain, DEFAULT_PATH)],
                }
            );
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("e\n{KEY_A}\no\n")
            );
        }

        #[test]
        fn w24_a_target_shrunk_below_the_group_s_start_is_not_extended() {
            let profile = Profile::new();
            with_keys_file(&profile, "existing\n");
            let (report, _) = profile.run_writing_with(
                "function _w([byte[]]$b){$s=[IO.File]::Open($f,'Open','Write','ReadWrite');\
                 $s.SetLength(1);$s.Dispose();0}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(
                report,
                Report {
                    outcome: Outcome::Uncertain,
                    keys: vec![key(1, KeyStatus::Uncertain, DEFAULT_PATH)],
                }
            );
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "e");
        }

        #[test]
        fn w19_an_unexpected_error_is_one_plain_line_and_no_summary() {
            let profile = Profile::new();
            let (report, stdout, stderr) = run_script(
                &script_with("$a=$false\nfunction New-Object{throw 'injected'}", ""),
                &profile.0,
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Unknown, "{stdout}");
            assert_eq!(
                stderr,
                "ssh-copy-id: ERROR: the installation script stopped: injected\r\n"
            );
        }

        /// Another process holding a lock on every byte of `path` from
        /// `offset` on, so that a write reaching it fails; released on drop.
        struct LockedFrom(std::process::Child);

        impl LockedFrom {
            fn new(path: &std::path::Path, offset: usize) -> LockedFrom {
                use std::io::{BufRead, BufReader};
                let script = format!(
                    "$s=[IO.File]::Open('{}','Open','ReadWrite','ReadWrite');\
                     $s.Lock({offset},1GB);[Console]::Out.WriteLine('locked');\
                     $null=[Console]::In.ReadLine()",
                    path.display()
                );
                let mut child = Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                    .arg(encoded(&script))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap();
                let mut line = String::new();
                BufReader::new(child.stdout.as_mut().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                assert_eq!(line.trim_end(), "locked");
                LockedFrom(child)
            }
        }

        impl Drop for LockedFrom {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }

        fn with_keys_file(profile: &Profile, existing: &str) {
            fs::create_dir_all(profile.file(".ssh")).unwrap();
            fs::write(profile.keys(), existing).unwrap();
        }

        #[test]
        fn w15_a_newline_added_for_a_failed_key_is_removed() {
            let profile = Profile::new();
            with_keys_file(&profile, "e");
            let lock = LockedFrom::new(&profile.keys(), 2);
            let (report, _) = profile.run(format!("{KEY_A}\n{KEY_B}\n").as_bytes());
            drop(lock);
            assert_eq!(
                report,
                unchanged(vec![
                    key(1, KeyStatus::Failed, DEFAULT_PATH),
                    key(2, KeyStatus::Failed, DEFAULT_PATH),
                ])
            );
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "e");
        }

        #[test]
        fn w16_a_comment_before_a_failed_key_is_rolled_back_with_it() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let lock = LockedFrom::new(&profile.keys(), "e\n# c\n".len());
            let (report, _) = profile.run(format!("# c\n{KEY_A}\n").as_bytes());
            drop(lock);
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, DEFAULT_PATH)])
            );
            assert_eq!(fs::read_to_string(profile.keys()).unwrap(), "e\n");
        }

        #[test]
        fn w17_keys_written_before_a_failure_stay_and_the_outcome_is_partial() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let lock = LockedFrom::new(&profile.keys(), format!("e\n{KEY_A}\n# c\n").len());
            let (report, _) = profile.run(format!("{KEY_A}\n# c\n{KEY_B}\n# d\n").as_bytes());
            drop(lock);
            assert_eq!(
                report,
                Report {
                    outcome: Outcome::Partial,
                    keys: vec![
                        key(1, KeyStatus::Added, DEFAULT_PATH),
                        key(2, KeyStatus::Failed, DEFAULT_PATH),
                    ],
                }
            );
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("e\n{KEY_A}\n")
            );
        }

        #[test]
        fn w18_a_failed_trailing_comment_is_rolled_back_and_the_outcome_is_partial() {
            let profile = Profile::new();
            with_keys_file(&profile, "e\n");
            let lock = LockedFrom::new(&profile.keys(), format!("e\n{KEY_A}\n").len());
            let (report, _) = profile.run(format!("{KEY_A}\n# last\n").as_bytes());
            drop(lock);
            assert_eq!(
                report,
                Report {
                    outcome: Outcome::Partial,
                    keys: vec![key(1, KeyStatus::Added, DEFAULT_PATH)],
                }
            );
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("e\n{KEY_A}\n")
            );
        }
    }
}
