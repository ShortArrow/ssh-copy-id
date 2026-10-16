//! The probe that tells a destination's shell family before any write, and the reading of its output.

/// The SSH command sent to detect the destination's shell family.
///
/// Each family expands a different part: `cmd.exe` only `%OS%`, PowerShell only
/// `$home`, and POSIX shells and `tcsh` `$0` (`tcsh` also `$home`).
pub const PROBE_COMMAND: &str = "echo ssh-copy-id-probe:%OS%:$0/$home";

/// Shell family of a destination, which decides the installation script it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Windows,
    UnixLike,
}

/// Reads the probe's standard output.
///
/// Returns `None` when no line starts with the marker, or when the first such
/// line has neither the Windows form nor the Unix-like form.
pub fn classify(stdout: &[u8]) -> Option<ShellFamily> {
    let line = stdout
        .split(|&b| b == b'\n')
        .find(|line| line.starts_with(MARKER))?;
    let rest = line[MARKER.len()..]
        .strip_suffix(b"\r")
        .unwrap_or(&line[MARKER.len()..]);
    if rest == CMD_FORM || is_powershell_form(rest) {
        Some(ShellFamily::Windows)
    } else if is_unix_form(rest) {
        Some(ShellFamily::UnixLike)
    } else {
        None
    }
}

const MARKER: &[u8] = b"ssh-copy-id-probe:";
/// `cmd.exe` expands `%OS%` and leaves the rest.
const CMD_FORM: &[u8] = b"Windows_NT:$0/$home";
/// PowerShell, `sh`, and `tcsh` leave `%OS%` as it is.
const UNEXPANDED_OS: &[u8] = b"%OS%:";

/// PowerShell expands only `$home`, which on Windows starts with a drive letter, a colon, and a backslash.
fn is_powershell_form(rest: &[u8]) -> bool {
    matches!(
        rest.strip_prefix(UNEXPANDED_OS),
        Some([b'/', drive, b':', b'\\', ..]) if drive.is_ascii_alphabetic()
    )
}

/// A POSIX shell or `tcsh` puts its bare name in `$0`, then `/` and whatever `$home` expands to.
fn is_unix_form(rest: &[u8]) -> bool {
    rest.strip_prefix(UNEXPANDED_OS)
        .and_then(|name_and_home| name_and_home.iter().position(|&b| b == b'/'))
        .is_some_and(|slash| slash > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o01_cmd_expands_only_os() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:Windows_NT:$0/$home\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o02_powershell_home_is_a_windows_path() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:/C:\\Users\\fixtureadmin\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o03_posix_shells_name_themselves() {
        for stdout in [
            &b"ssh-copy-id-probe:%OS%:sh/\n"[..],
            b"ssh-copy-id-probe:%OS%:dash/\n",
            b"ssh-copy-id-probe:%OS%:bash/\n",
        ] {
            assert_eq!(classify(stdout), Some(ShellFamily::UnixLike));
        }
    }

    #[test]
    fn o04_tcsh_also_expands_home() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:tcsh//home/keyuser\n"),
            Some(ShellFamily::UnixLike)
        );
    }

    #[test]
    fn o05_pwsh_as_a_linux_login_shell_is_unknown() {
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%://home/keyuser\n"), None);
    }

    #[test]
    fn o06_lines_printed_by_startup_files_come_first() {
        assert_eq!(
            classify(b"startup-line\nssh-copy-id-probe:%OS%:bash/\n"),
            Some(ShellFamily::UnixLike)
        );
        assert_eq!(
            classify(b"startup-line\r\nssh-copy-id-probe:Windows_NT:$0/$home\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o07_only_the_first_marker_line_counts() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%://home/keyuser\nssh-copy-id-probe:%OS%:bash/\n"),
            None
        );
    }

    #[test]
    fn o08_output_without_a_marker_line_is_unknown() {
        assert_eq!(classify(b""), None);
        assert_eq!(classify(b"startup-line\n"), None);
        assert_eq!(classify(b"x ssh-copy-id-probe:%OS%:bash/\n"), None);
    }

    #[test]
    fn o09_wsl_exe_prints_utf16_without_a_marker_line() {
        let stdout: Vec<u8> = "Invalid command line argument: -c\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(classify(&stdout), None);
    }

    #[test]
    fn o10_bash_exe_passes_the_shell_path_in_argv0() {
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%:/bin/bash/\n"), None);
    }

    #[test]
    fn o11_line_without_the_trailing_slash_is_unknown() {
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%:bash\n"), None);
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%:/\n"), None);
    }

    #[test]
    fn o12_windows_form_needs_drive_colon_and_backslash() {
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%:/C:/Users/x\r\n"), None);
        assert_eq!(classify(b"ssh-copy-id-probe:%OS%:/C\r\n"), None);
        assert_eq!(classify(b"ssh-copy-id-probe:Windows_NT:bash/\r\n"), None);
    }

    #[test]
    fn o13_last_line_without_a_newline_is_read() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:sh/"),
            Some(ShellFamily::UnixLike)
        );
    }
}
