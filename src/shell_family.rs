//! The probe that tells a destination's shell family before any write, and the reading of its output.

/// The SSH command sent to detect the destination's shell family.
///
/// `cmd.exe` expands only `%OS%`, and PowerShell only `$home`, to a Windows path.
pub const PROBE_COMMAND: &str = "echo ssh-copy-id-probe:%OS%:$home";

/// Shell family of a destination, which decides the installation script it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Windows,
    UnixLike,
}

/// Reads the probe's standard output.
///
/// Returns `None` when no line starts with the marker. The first such line is
/// Windows in the form `cmd.exe` or PowerShell on Windows prints, and Unix-like
/// otherwise, as upstream takes every destination to be.
pub fn classify(stdout: &[u8]) -> Option<ShellFamily> {
    let line = stdout
        .split(|&b| b == b'\n')
        .find(|line| line.starts_with(MARKER))?;
    let rest = line[MARKER.len()..]
        .strip_suffix(b"\r")
        .unwrap_or(&line[MARKER.len()..]);
    if rest == CMD_FORM || is_powershell_form(rest) {
        Some(ShellFamily::Windows)
    } else {
        Some(ShellFamily::UnixLike)
    }
}

const MARKER: &[u8] = b"ssh-copy-id-probe:";
/// `cmd.exe` expands `%OS%` and leaves the rest.
const CMD_FORM: &[u8] = b"Windows_NT:$home";

/// PowerShell leaves `%OS%` and expands `$home`, which on Windows starts with a drive letter, a colon, and a backslash.
fn is_powershell_form(rest: &[u8]) -> bool {
    matches!(
        rest.strip_prefix(b"%OS%:"),
        Some([drive, b':', b'\\', ..]) if drive.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o00_the_probe_names_os_and_home() {
        assert_eq!(PROBE_COMMAND, "echo ssh-copy-id-probe:%OS%:$home");
    }

    #[test]
    fn o01_cmd_expands_only_os() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:Windows_NT:$home\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o02_powershell_home_is_a_windows_path() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:C:\\Users\\fixtureadmin\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o03_posix_shells_expand_nothing() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:\n"),
            Some(ShellFamily::UnixLike)
        );
    }

    #[test]
    fn o04_csh_tcsh_and_linux_pwsh_expand_a_unix_home() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:/home/keyuser\n"),
            Some(ShellFamily::UnixLike)
        );
    }

    #[test]
    fn o05_any_other_marker_line_is_unix_like() {
        for stdout in [
            &b"ssh-copy-id-probe:%OS%:C:/Users/x\r\n"[..],
            b"ssh-copy-id-probe:%OS%:C\r\n",
            b"ssh-copy-id-probe:Windows_NT:bash\r\n",
            b"ssh-copy-id-probe:%OS%:/bin/sh/\n",
            b"ssh-copy-id-probe:\n",
        ] {
            assert_eq!(classify(stdout), Some(ShellFamily::UnixLike));
        }
    }

    #[test]
    fn o06_lines_printed_by_startup_files_come_first() {
        assert_eq!(
            classify(b"startup-line\nssh-copy-id-probe:%OS%:C:\\Users\\x\r\n"),
            Some(ShellFamily::Windows)
        );
        assert_eq!(
            classify(b"startup-line\r\nssh-copy-id-probe:Windows_NT:$home\r\n"),
            Some(ShellFamily::Windows)
        );
    }

    #[test]
    fn o07_only_the_first_marker_line_counts() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:%OS%:/home/k\nssh-copy-id-probe:%OS%:C:\\Users\\x\r\n"),
            Some(ShellFamily::UnixLike)
        );
    }

    #[test]
    fn o08_output_without_a_marker_line_is_none() {
        assert_eq!(classify(b""), None);
        assert_eq!(classify(b"\n"), None);
        assert_eq!(classify(b"startup-line\n"), None);
        assert_eq!(classify(b"x ssh-copy-id-probe:Windows_NT:$home\r\n"), None);
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
    fn o10_last_line_without_a_newline_is_read() {
        assert_eq!(
            classify(b"ssh-copy-id-probe:Windows_NT:$home"),
            Some(ShellFamily::Windows)
        );
    }
}
