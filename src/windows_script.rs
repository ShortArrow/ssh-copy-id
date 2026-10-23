//! The installation command sent to Windows destinations.

use crate::remote_script::TargetPath;

/// Builds `powershell.exe -NoProfile -NonInteractive -EncodedCommand <base64>`,
/// whose loader runs the installation script that [`windows_install_input`]
/// puts on stdin.
///
/// The command holds only letters, digits and base64, so `cmd.exe /c`,
/// `powershell.exe -c` and `pwsh -c` pass it unchanged, and it fits in the
/// 8191 characters `cmd.exe` accepts whatever the script holds.
pub fn windows_install_command() -> String {
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encoded(LOADER)
    )
}

/// The stdin of [`windows_install_command`]: the installation script, a NUL
/// byte, then `text`, the key lines, which hold no NUL.
///
/// The script appends the key lines to `.ssh\authorized_keys` under the
/// profile directory and prints the result lines that
/// `result_line::parse_report` reads, as `remote_script::install_command`'s
/// does. It takes the lines as bytes and appends every line byte for byte, a CR
/// at its end and comment and blank lines included, each followed by LF; only
/// the other lines are keys. Comment and blank lines, the newline added after
/// a last line without one, the groups of lines, the rollback of a failed
/// group and the reported results follow `remote_script::install_command`.
/// The rollback compares the bytes past the group's starting size with the
/// bytes the run tried to write; anything else is left and reported
/// `uncertain`. A missing directory or file is created with the ACL it
/// inherits. Each write goes through a handle that may only append, so runs
/// at the same time do not overwrite each other.
///
/// An account whose token holds the Administrators group, SID `S-1-5-32-544`,
/// even for deny only, writes to `administrators_authorized_keys` under
/// `$env:ProgramData\ssh` instead and gets that full path in the result lines.
/// Before writing, the script names the file on stderr as shared by every
/// administrator account. A missing file is created empty with only
/// Administrators and SYSTEM allowed full control and no inherited entries,
/// before any key goes in, and removed again when its first group fails; an
/// existing file keeps its ACL. A missing `$env:ProgramData\ssh` is not
/// created and gets nothing written.
///
/// `target`, the `-t` path, replaces both files for every account and prints
/// no shared-file notice. A fully qualified path, a drive letter followed by
/// `\` or `/` or a path starting with `\\`, is used as given, and any other
/// path is relative to the profile; nothing in it is expanded. A path rooted
/// without a drive, or a drive without a root, gets nothing written. Missing
/// parent directories are created with the ACL they inherit, and a missing file
/// is created with only the account, SYSTEM and Administrators allowed full
/// control and no inherited entries; an existing file keeps its ACL. The
/// result lines carry the path as given.
///
/// A target that is a directory, and a non-empty target that cannot be read,
/// get nothing written and every key reported failed. A `$HOME` that is not a
/// directory also gets nothing written, so that nothing goes to the working
/// directory; PowerShell creates a missing profile directory before the script
/// runs.
pub fn windows_install_input(target: Option<&TargetPath>, text: &[u8]) -> Vec<u8> {
    framed(
        &format!(
            "{ADMIN_CHECK}\n{}\n{INSTALL_FUNCTIONS}\n{INSTALL_MAIN}",
            target_line(target)
        ),
        text,
    )
}

/// Sets `$T` to the `-t` path, or to `$null` without one. The path is a
/// base64 literal of its UTF-8 bytes, so the script stays ASCII and every
/// character of the path is data.
fn target_line(target: Option<&TargetPath>) -> String {
    match target {
        Some(path) => format!(
            "$T=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}'))",
            base64(path.as_str().as_bytes())
        ),
        None => "$T=$null".to_string(),
    }
}

/// File name of the key file every administrator account shares, under
/// `$env:ProgramData\ssh`, which the script reports for an administrator.
pub const ADMIN_KEY_FILE: &str = "administrators_authorized_keys";

/// `script` and `text`, each followed by a NUL byte, as [`LOADER`] reads them.
fn framed(script: &str, text: &[u8]) -> Vec<u8> {
    let mut input = script.as_bytes().to_vec();
    input.push(0);
    input.extend_from_slice(text);
    input.push(0);
    input
}

/// Reads stdin up to its second NUL, runs the ASCII text before the first NUL
/// as the script, dot-sourced, and leaves the bytes between the two in `$i`.
/// It never waits for the end of stdin, which sshd on Windows may not deliver
/// once the input passes about 4 KB, and runs nothing when stdin ends before
/// the second NUL. Stdin is read as bytes, not through PowerShell's input
/// encoding, which follows the console code page.
const LOADER: &str = "$ProgressPreference='SilentlyContinue';$q=[Console]::OpenStandardInput();$b=New-Object byte[] 65536;$m=New-Object IO.MemoryStream;$e=-1;$n=-1;while($n -lt 0){$c=$q.Read($b,0,$b.Length);if($c -le 0){exit 1};$m.Write($b,0,$c);$i=$m.ToArray();$e=[Array]::IndexOf($i,[byte]0);if($e -ge 0){$n=[Array]::IndexOf($i,[byte]0,$e+1)}};$s=[Text.Encoding]::ASCII.GetString($i,0,$e);$k=New-Object byte[] ($n-$e-1);[Array]::Copy($i,$e+1,$k,0,$k.Length);$i=$k;. ([ScriptBlock]::Create($s))";

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
function _w([byte[]]$b){try{$s=New-Object IO.FileStream($f,'Append',[Security.AccessControl.FileSystemRights]::AppendData,'ReadWrite',4096,'None');try{$s.Write($b,0,$b.Length);$s.Flush()}finally{$s.Dispose()};1}catch{0}}
function _l{$s=New-Object IO.FileStream($f,'Open','Read','ReadWrite',1);try{$null=$s.Seek(-1,'End');$s.ReadByte()}finally{$s.Dispose()}}
function _a([byte[]]$l){try{if(!$w -and (_s) -gt 0 -and (_l) -ne 10){$t.Add(10);if(!(_w @(10))){return 0}};$t.AddRange($l);$t.Add(10);_w ([byte[]]($l+[byte[]]@(10)))}catch{0}}
function _c($m){[IO.File]::Open($f,'CreateNew','Write','ReadWrite').Dispose();try{$s=New-Object Security.AccessControl.FileSecurity;$s.SetAccessRuleProtection($true,$false);$m|%{$s.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier $_),'FullControl','Allow')))};[IO.File]::SetAccessControl($f,$s)}catch{[IO.File]::Delete($f);throw}}
function _r{try{$n=_s;if($n -eq $null){return $z -eq $null};$b=[long]$z;$k=New-Object byte[] ($n-$b);$s=New-Object IO.FileStream($f,'Open','Read','ReadWrite',1);try{$null=$s.Seek($b,'Begin');$q=0;while($q -lt $k.Length){$c=$s.Read($k,$q,$k.Length-$q);if(!$c){break};$q+=$c}}finally{$s.Dispose()};for($j=0;$j -lt $k.Length;$j++){if($k[$j] -ne $t[$j]){return $false}};if($z -eq $null){[IO.File]::Delete($f)}else{$s=[IO.File]::Open($f,'Open','Write','ReadWrite');try{$s.SetLength($z)}finally{$s.Dispose()}};(_s) -eq $z}catch{$false}}"#;

/// Appends the lines in `$i` to the `-t` path in `$T`, or else to the profile's
/// key file, or to the shared administrator file when `$a` is set, and reports
/// them. `$m` holds the SIDs a created file allows, `$null` where a created
/// file keeps the ACL it inherits.
/// `$x` is set once nothing more may be written, `$z` holds the target's size
/// before the current group (`$null` when it did not exist) and `$t` the bytes
/// tried in the group.
const INSTALL_MAIN: &str = r#"$f=[IO.Path]::Combine($HOME,'.ssh','authorized_keys');$p='.ssh/authorized_keys';$x=0;$d=0;$m=$null
if($T -ne $null){$a=$false;$p=_e $T;$m=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value,'S-1-5-18','S-1-5-32-544';if($T -match '^([A-Za-z]:[\\/]|\\\\)'){$f=$T}elseif([IO.Path]::IsPathRooted($T)){$x=1}else{$f=[IO.Path]::Combine($HOME,$T)}}
elseif($a){$f=[IO.Path]::Combine($env:ProgramData,'ssh','administrators_authorized_keys');$p=_e $f;$m='S-1-5-32-544','S-1-5-18'}
if(!$x){try{if($a){if(![IO.Path]::IsPathRooted($env:ProgramData) -or ![IO.Directory]::Exists([IO.Path]::GetDirectoryName($f))){throw 1}}else{if(![IO.Directory]::Exists($HOME)){throw 1};$null=[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($f))};if((_s) -gt 0){[IO.File]::Open($f,'Open','Read','ReadWrite').Dispose()}}catch{$x=1}}
if(!$x -and $a){[Console]::Error.WriteLine("ssh-copy-id: INFO: writing to $f, which every administrator account on this host shares")}
if(!$x -and $m){try{if($null -eq (_s)){_c $m;$d=1}}catch{$x=1}}
$n=0;$g=0;$w=0;$u=0;$h='failed';$t=New-Object Collections.Generic.List[byte];$z=$null;if(!$x -and !$d){$z=_s}
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

    const PREFIX: &str = "powershell.exe -NoProfile -NonInteractive -EncodedCommand ";

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
    fn w03_the_payload_is_the_utf16le_loader() {
        let command = windows_install_command();
        let bytes = decoded(command.strip_prefix(PREFIX).unwrap());
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), LOADER);
        assert!(!LOADER.is_empty());
    }

    #[test]
    fn w30_the_input_is_the_script_and_the_key_lines_each_ended_by_nul() {
        let mut expected = script_with(&format!("{ADMIN_CHECK}\n$T=$null"), "").into_bytes();
        expected.push(0);
        expected.extend_from_slice(b"k1\n# c\r\n");
        expected.push(0);
        assert_eq!(windows_install_input(None, b"k1\n# c\r\n"), expected);
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
        assert!(
            INSTALL_MAIN.is_ascii()
                && INSTALL_FUNCTIONS.is_ascii()
                && ADMIN_CHECK.is_ascii()
                && LOADER.is_ascii()
        );
        assert!(!script_with(ADMIN_CHECK, "").contains('\0'));
    }

    #[test]
    fn w35_a_target_is_a_base64_literal_of_its_utf8_bytes() {
        let target = TargetPath::new("鍵\\it's").unwrap();
        let input = windows_install_input(Some(&target), b"k1\n");
        let script = String::from_utf8(input[..input.len() - 5].to_vec()).unwrap();
        assert!(script.is_ascii());
        assert!(
            script.contains(&format!(
                "\n$T=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}'))\n",
                base64("鍵\\it's".as_bytes())
            )),
            "{script}"
        );
    }

    #[test]
    fn w34_the_script_names_the_shared_file_admin_key_file_names() {
        assert!(INSTALL_MAIN.contains(&format!("'ssh','{ADMIN_KEY_FILE}'")));
    }

    #[cfg(windows)]
    mod run {
        use super::*;
        use crate::result_line::{KeyResult, KeyStatus, Outcome, Report, parse_report};
        use std::fs;
        use std::path::PathBuf;
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicUsize, Ordering};

        const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== a@here";
        const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB== b@here";
        const DEFAULT_PATH: &[u8] = b".ssh/authorized_keys";

        static NEXT: AtomicUsize = AtomicUsize::new(0);

        fn key(index: usize, status: KeyStatus, path: &[u8]) -> KeyResult {
            KeyResult {
                index,
                status,
                path: path.to_vec(),
            }
        }

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
                self.run_as_with(admin, "", stdin)
            }

            /// Runs the script as `run_as` does, with the definitions in
            /// `between` in place of the script's own.
            fn run_as_with(
                &self,
                admin: bool,
                between: &str,
                stdin: &[u8],
            ) -> (Report, String, String) {
                let flag = if admin { "$true" } else { "$false" };
                run_script(&script_with(&format!("$a={flag}"), between), &self.0, stdin)
            }

            /// `ssh` under the profile's stand-in for `$env:ProgramData`.
            fn shared_dir(&self) -> PathBuf {
                self.file("ProgramData\\ssh")
            }

            fn shared(&self) -> PathBuf {
                self.shared_dir().join("administrators_authorized_keys")
            }

            /// The shared file's path as the result lines carry it.
            fn shared_path(&self) -> Vec<u8> {
                crate::result_line::encode_path(self.shared().to_string_lossy().as_bytes())
                    .into_bytes()
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

            /// Runs the script as `run_as_with` does, with `target` as the
            /// `-t` path.
            fn run_target(
                &self,
                admin: bool,
                target: &str,
                between: &str,
                stdin: &[u8],
            ) -> (Report, String, String) {
                let flag = if admin { "$true" } else { "$false" };
                let line = target_line(Some(&TargetPath::new(target).unwrap()));
                run_script(
                    &script_with(&format!("$a={flag}\n{line}"), between),
                    &self.0,
                    stdin,
                )
            }
        }

        /// The DACL of `path` in SDDL.
        fn sddl(path: &std::path::Path) -> String {
            powershell(&format!(
                "[Console]::Out.Write((Get-Item -LiteralPath '{}').GetAccessControl('Access').GetSecurityDescriptorSddlForm('Access'))",
                path.display()
            ))
        }

        /// The access rules of `path`, each as its SID, rights and type,
        /// sorted; SDDL would print a well-known account by its alias.
        fn access_rules(path: &std::path::Path) -> Vec<String> {
            let mut rules: Vec<String> = powershell(&format!(
                "(Get-Item -LiteralPath '{}').GetAccessControl('Access').GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])|%{{[Console]::Out.Write(\"$($_.IdentityReference) $($_.FileSystemRights) $($_.AccessControlType)`n\")}}",
                path.display()
            ))
            .lines()
            .map(str::to_string)
            .collect();
            rules.sort();
            rules
        }

        /// The SID of the account running the tests.
        fn user_sid() -> String {
            powershell(
                "[Console]::Out.Write([Security.Principal.WindowsIdentity]::GetCurrent().User.Value)",
            )
        }

        fn powershell(script: &str) -> String {
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encoded(script))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        #[test]
        fn w36_a_relative_target_is_created_under_the_profile_with_its_parents() {
            let profile = Profile::new();
            let target = "keys\\it's 鍵";
            let (report, stdout, stderr) =
                profile.run_target(false, target, "", format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, target.as_bytes())]),
                "{stdout}"
            );
            assert_eq!(
                fs::read_to_string(profile.file(target)).unwrap(),
                format!("{KEY_A}\n")
            );
            assert_eq!(stderr, "");
        }

        #[test]
        fn w37_a_created_key_file_allows_only_the_account_system_and_administrators() {
            let profile = Profile::new();
            let target = profile.file("outside\\deep\\keys");
            let (report, stdout, _) = profile.run_target(
                false,
                &target.display().to_string(),
                "",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
            let file = sddl(&target);
            assert!(file.starts_with("D:PAI"), "{file}");
            let mut expected = vec![
                format!("{} FullControl Allow", user_sid()),
                "S-1-5-18 FullControl Allow".to_string(),
                "S-1-5-32-544 FullControl Allow".to_string(),
            ];
            expected.sort();
            assert_eq!(access_rules(&target), expected, "{file}");
            let directory = sddl(&profile.file("outside\\deep"));
            assert!(!directory.starts_with("D:P"), "{directory}");
        }

        #[test]
        fn w38_an_administrator_s_target_is_the_named_file_without_the_shared_notice() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            let (report, stdout, stderr) = profile.run_target(
                true,
                ".ssh\\authorized_keys",
                "",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
            assert_eq!(
                fs::read_to_string(profile.keys()).unwrap(),
                format!("{KEY_A}\n")
            );
            assert!(!profile.shared().exists());
            assert_eq!(stderr, "");
        }

        #[test]
        fn w39_a_path_that_depends_on_the_current_drive_gets_nothing_written() {
            let profile = Profile::new();
            let name = format!("ssh-copy-id-w39-{}", std::process::id());
            for target in [format!("\\{name}"), format!("C:{name}")] {
                let (report, stdout, _) =
                    profile.run_target(false, &target, "", format!("{KEY_A}\n").as_bytes());
                assert_eq!(
                    report,
                    unchanged(vec![key(1, KeyStatus::Failed, target.as_bytes())]),
                    "{target}: {stdout}"
                );
                assert!(!PathBuf::from(format!("C:\\{name}")).exists());
                assert!(!profile.file(&name).exists());
                assert!(!profile.file(".ssh").exists());
            }
        }

        #[test]
        fn w40_an_existing_target_keeps_its_acl() {
            let profile = Profile::new();
            fs::write(profile.file("keys"), "e\n").unwrap();
            let before = sddl(&profile.file("keys"));
            let (report, stdout, _) = profile.run_target(
                false,
                "keys",
                "function _c{throw 'created'}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
            assert_eq!(sddl(&profile.file("keys")), before);
            assert_eq!(
                fs::read_to_string(profile.file("keys")).unwrap(),
                format!("e\n{KEY_A}\n")
            );
        }

        #[test]
        fn w42_a_path_starting_with_two_backslashes_is_used_as_given() {
            let profile = Profile::new();
            let target = format!("\\\\?\\{}", profile.file("unc\\keys").display());
            let (report, stdout, _) =
                profile.run_target(false, &target, "", format!("{KEY_A}\n").as_bytes());
            assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
            assert_eq!(
                fs::read_to_string(profile.file("unc\\keys")).unwrap(),
                format!("{KEY_A}\n")
            );
        }

        #[test]
        fn w41_a_variable_in_the_target_is_not_expanded() {
            let profile = Profile::new();
            let (report, stdout, _) = profile.run_target(
                false,
                "%USERPROFILE%\\$HOME",
                "",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
            assert!(profile.file("%USERPROFILE%\\$HOME").is_file());
        }

        impl Drop for Profile {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        /// Runs `script` through the loader with `stdin` as the key lines,
        /// keeping stdin open until the run ends, as sshd on Windows may.
        fn run_script(
            script: &str,
            profile: &std::path::Path,
            stdin: &[u8],
        ) -> (Report, String, String) {
            use std::io::Write;
            let mut child = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encoded(LOADER))
                .env("USERPROFILE", profile)
                .env("ProgramData", profile.join("ProgramData"))
                .current_dir(profile)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("powershell.exe must be on PATH for these tests");
            let mut open_stdin = child.stdin.take().unwrap();
            open_stdin.write_all(&framed(script, stdin)).unwrap();
            let output = child.wait_with_output().unwrap();
            drop(open_stdin);
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
        fn w32_runs_appending_at_the_same_time_keep_every_line() {
            let profile = Profile::new();
            with_keys_file(&profile, "");
            let lines: Vec<String> = (0..8).map(|n| format!("{KEY_A} run{n}\n")).collect();
            std::thread::scope(|scope| {
                for line in &lines {
                    let profile = &profile;
                    scope.spawn(move || {
                        let (report, stdout) = profile.run(line.as_bytes());
                        assert_eq!(report.outcome, Outcome::Installed, "{stdout}");
                    });
                }
            });
            let written = fs::read_to_string(profile.keys()).unwrap();
            for line in &lines {
                assert!(
                    written.contains(line.as_str()),
                    "{line} missing from:\n{written}"
                );
            }
            assert_eq!(written.len(), lines.iter().map(String::len).sum::<usize>());
        }

        #[test]
        fn w33_a_relative_programdata_gets_nothing_written() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            let (report, _, stderr) = profile.run_as_with(
                true,
                "$env:ProgramData='ProgramData'",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(report.outcome, Outcome::Unchanged);
            assert_eq!(report.keys.len(), 1);
            assert_eq!(report.keys[0].status, KeyStatus::Failed);
            assert_eq!(stderr, "");
            assert!(!profile.shared().exists());
        }

        #[test]
        fn w31_key_lines_far_past_the_pipe_buffer_are_all_read() {
            let profile = Profile::new();
            let comment = format!("# {}\n", "c".repeat(1000));
            let input = format!("{}{KEY_A}\n", comment.repeat(200));
            let (report, stdout) = profile.run(input.as_bytes());
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, DEFAULT_PATH)]),
                "{stdout}"
            );
            assert_eq!(fs::read(profile.keys()).unwrap(), input.as_bytes());
        }

        #[test]
        fn w25_a_successful_installation_writes_nothing_to_stderr() {
            let profile = Profile::new();
            let (report, _, stderr) = profile.run_as(false, format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, DEFAULT_PATH)])
            );
            assert_eq!(stderr, "");
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
        fn w10_an_administrator_s_keys_go_to_the_shared_file_after_its_creation() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            let (report, stdout, stderr) = profile.run_as_with(
                true,
                "function _c{[IO.File]::WriteAllText($f,\"# created`n\")}",
                format!("{KEY_A}\n{KEY_B}\n").as_bytes(),
            );
            assert_eq!(
                report,
                installed(vec![
                    key(1, KeyStatus::Added, &profile.shared_path()),
                    key(2, KeyStatus::Added, &profile.shared_path()),
                ]),
                "{stdout}"
            );
            assert_eq!(
                fs::read_to_string(profile.shared()).unwrap(),
                format!("# created\n{KEY_A}\n{KEY_B}\n")
            );
            assert_eq!(
                stderr,
                format!(
                    "ssh-copy-id: INFO: writing to {}, which every administrator account \
                     on this host shares\r\n",
                    profile.shared().display()
                )
            );
            assert!(!profile.file(".ssh").exists());
        }

        #[test]
        fn w26_an_existing_shared_file_is_appended_to_without_creating_it() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            fs::write(profile.shared(), "e\n").unwrap();
            let (report, stdout, _) = profile.run_as_with(
                true,
                "function _c{throw 'created'}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(
                report,
                installed(vec![key(1, KeyStatus::Added, &profile.shared_path())]),
                "{stdout}"
            );
            assert_eq!(
                fs::read_to_string(profile.shared()).unwrap(),
                format!("e\n{KEY_A}\n")
            );
        }

        #[test]
        fn w27_a_missing_programdata_ssh_gets_nothing_written_or_announced() {
            let profile = Profile::new();
            let (report, _, stderr) = profile.run_as(true, format!("{KEY_A}\n").as_bytes());
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, &profile.shared_path())])
            );
            assert_eq!(stderr, "");
            assert!(!profile.shared_dir().exists());
            assert!(!profile.file(".ssh").exists());
        }

        #[test]
        fn w28_a_created_shared_file_is_empty_and_allows_only_administrators_and_system() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            let path = profile.shared().display().to_string();
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encoded(&format!(
                    "{INSTALL_FUNCTIONS}\n$f='{path}';_c 'S-1-5-32-544','S-1-5-18'\n\
                     [Console]::Out.Write([IO.File]::GetAccessControl($f).GetSecurityDescriptorSddlForm('Access'))"
                )))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "D:PAI(A;;FA;;;SY)(A;;FA;;;BA)",
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::metadata(profile.shared()).unwrap().len(), 0);
        }

        #[test]
        fn w29_a_shared_file_created_for_a_failed_group_is_removed() {
            let profile = Profile::new();
            fs::create_dir_all(profile.shared_dir()).unwrap();
            let (report, stdout, _) = profile.run_as_with(
                true,
                "function _c{[IO.File]::Open($f,'CreateNew','Write','ReadWrite').Dispose()}\n\
                 function _w([byte[]]$b){0}",
                format!("{KEY_A}\n").as_bytes(),
            );
            assert_eq!(
                report,
                unchanged(vec![key(1, KeyStatus::Failed, &profile.shared_path())]),
                "{stdout}"
            );
            assert!(!profile.shared().exists());
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
