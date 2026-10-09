//! The `ssh-copy-id` command.

use ssh_copy_id::app::{self, Environment};
use ssh_copy_id::cli_args::{self, ArgsError};
use ssh_copy_id::platform;
use ssh_copy_id::ssh_process::SystemSsh;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage: ssh-copy-id [-h|-?|-f|-n] [-i [identity_file]] [-t target_path] [-p port] [-F ssh_config] [[-o ssh_option] ...] [user@]hostname
\t-f: force mode -- copy keys without trying to check if they are already installed
\t-n: dry run    -- no keys are actually copied
\t-i: the public key to install; '.pub' is added when absent
\t-t: the remote file to add the keys to, relative to the home directory
\t-p: port of the remote host
\t-F, -o: passed to ssh unchanged
\t-h|-?: print this help
This release installs keys on a Unix-like host.
-s and -x are not available yet.";

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                eprintln!("ssh-copy-id: ERROR: argument is not valid Unicode: {arg:?}");
                return ExitCode::from(1);
            }
        }
    }
    let read_file = |path: &std::path::Path| std::fs::read(path);
    let invocation =
        match cli_args::parse(&args, &|argument| names_a_key_file(argument, &read_file)) {
            Ok(invocation) => invocation,
            Err(error) => {
                eprint!("{}", preamble(&error));
                eprintln!("{USAGE}");
                return ExitCode::from(1);
            }
        };
    platform::outlive_interrupts();
    let exists = |path: &std::path::Path| path.exists();
    let same_file = |a: &std::path::Path, b: &std::path::Path| match (
        std::fs::canonicalize(a),
        std::fs::canonicalize(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let env = Environment {
        has_console: platform::has_console(),
        askpass_set: std::env::var_os("SSH_ASKPASS").is_some(),
        home: platform::home_dir(),
        read_file: &read_file,
        exists: &exists,
        readable_file: &platform::open_readable_file,
        same_file: &same_file,
        modification_times: &platform::modification_times,
        create_scratch_dir: &platform::create_scratch_dir,
        write_file: &|path, contents| std::fs::write(path, contents),
        remove_dir: &platform::remove_scratch_dir,
        interrupted: &platform::interrupted,
    };
    let status = app::run(
        &invocation,
        &env,
        &mut SystemSsh::new(),
        &mut io::stdout(),
        &mut io::stderr(),
    );
    let _ = io::stdout().flush();
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}

/// The text printed to stderr before the usage, in upstream's form where upstream
/// rejects the same arguments: nothing for `-h`, `-?` and a missing destination,
/// bash's `getopts` line for an unknown letter or a missing value, and
/// upstream's blank lines around a repeated `-i` and a missing hostname.
fn preamble(error: &ArgsError) -> String {
    match error {
        ArgsError::Help | ArgsError::NoDestination => String::new(),
        ArgsError::IllegalOption(letter) => format!("ssh-copy-id: illegal option -- {letter}\n"),
        ArgsError::MissingValue(flag) => {
            format!("ssh-copy-id: option requires an argument -- {flag}\n")
        }
        ArgsError::RepeatedIdentity => {
            between_blank_lines("-i option must not be specified more than once")
        }
        ArgsError::MissingHostname(argument) => between_blank_lines(&format!(
            "Missing hostname. Use \"-i -- {argument}\" if you really mean to use this as the hostname"
        )),
        ArgsError::TooManyArguments(extra) => {
            before_blank_line(&format!("Too many arguments: {}", extra.join(" ")))
        }
        ArgsError::Unsupported(flag) => {
            before_blank_line(&format!("option -{flag} is not available in this release"))
        }
        ArgsError::Unknown(option) => before_blank_line(&format!("unknown option {option}")),
    }
}

fn between_blank_lines(message: &str) -> String {
    format!("\nssh-copy-id: ERROR: {message}\n\n")
}

fn before_blank_line(message: &str) -> String {
    format!("ssh-copy-id: ERROR: {message}\n\n")
}

/// Upstream's `[ -r "$arg" ] && grep -iq ssh "$arg"`: the argument is a readable file containing `ssh` in any case.
fn names_a_key_file(argument: &str, read_file: &dyn Fn(&Path) -> io::Result<Vec<u8>>) -> bool {
    read_file(Path::new(argument)).is_ok_and(|content| {
        content
            .windows(3)
            .any(|window| window.eq_ignore_ascii_case(b"ssh"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m01_readable_file_containing_ssh_names_a_key_file() {
        let read = |path: &Path| {
            assert_eq!(path, Path::new("k.pub"));
            Ok(b"ssh-ed25519 AAAA user@laptop\n".to_vec())
        };
        assert!(names_a_key_file("k.pub", &read));
    }

    #[test]
    fn m02_ssh_is_matched_without_case() {
        let read = |_: &Path| Ok(b"key from SSH agent".to_vec());
        assert!(names_a_key_file("k", &read));
    }

    #[test]
    fn m03_file_without_ssh_does_not_name_a_key_file() {
        let read = |_: &Path| Ok(b"not a key".to_vec());
        assert!(!names_a_key_file("host", &read));
    }

    #[test]
    fn m04_unreadable_argument_does_not_name_a_key_file() {
        let read = |_: &Path| Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(!names_a_key_file("host", &read));
    }

    #[test]
    fn m05_unknown_long_option_is_named() {
        assert_eq!(
            preamble(&ArgsError::Unknown("--target-os".into())),
            "ssh-copy-id: ERROR: unknown option --target-os\n\n"
        );
    }

    #[test]
    fn m06_no_destination_prints_only_the_usage() {
        assert_eq!(preamble(&ArgsError::NoDestination), "");
    }

    #[test]
    fn m07_help_prints_only_the_usage() {
        assert_eq!(preamble(&ArgsError::Help), "");
    }

    #[test]
    fn m08_an_unknown_letter_is_reported_as_bash_getopts_does() {
        assert_eq!(
            preamble(&ArgsError::IllegalOption('z')),
            "ssh-copy-id: illegal option -- z\n"
        );
    }

    #[test]
    fn m09_a_missing_value_is_reported_as_bash_getopts_does() {
        assert_eq!(
            preamble(&ArgsError::MissingValue('p')),
            "ssh-copy-id: option requires an argument -- p\n"
        );
    }

    #[test]
    fn m10_a_repeated_identity_is_framed_by_blank_lines_as_upstream() {
        assert_eq!(
            preamble(&ArgsError::RepeatedIdentity),
            "\nssh-copy-id: ERROR: -i option must not be specified more than once\n\n"
        );
    }

    #[test]
    fn m11_an_unsupported_option_keeps_the_stage_1_message() {
        assert_eq!(
            preamble(&ArgsError::Unsupported('s')),
            "ssh-copy-id: ERROR: option -s is not available in this release\n\n"
        );
    }

    #[test]
    fn m12_a_missing_hostname_suggests_the_double_dash_as_upstream() {
        assert_eq!(
            preamble(&ArgsError::MissingHostname("k.pub".into())),
            "\nssh-copy-id: ERROR: Missing hostname. Use \"-i -- k.pub\" if you really mean \
             to use this as the hostname\n\n"
        );
    }

    #[test]
    fn m13_the_usage_offers_identity_without_a_file() {
        assert!(USAGE.contains("[-i [identity_file]]"), "{USAGE}");
        assert!(!USAGE.contains("-i without a file"), "{USAGE}");
    }

    #[test]
    fn m14_the_usage_offers_force_mode_and_dry_run_in_upstream_s_words() {
        assert!(
            USAGE.starts_with("Usage: ssh-copy-id [-h|-?|-f|-n] [-i [identity_file]]"),
            "{USAGE}"
        );
        assert!(
            USAGE.contains(
                "\n\t-f: force mode -- copy keys without trying to check if they are already installed\n\
                 \t-n: dry run    -- no keys are actually copied\n"
            ),
            "{USAGE}"
        );
        assert!(
            USAGE.ends_with("\n-s and -x are not available yet."),
            "{USAGE}"
        );
    }

    #[test]
    fn m15_the_usage_offers_a_target_path_where_upstream_s_does() {
        assert!(
            USAGE.contains("[-i [identity_file]] [-t target_path] "),
            "{USAGE}"
        );
        assert!(
            USAGE.contains(
                "\n\t-t: the remote file to add the keys to, relative to the home directory\n"
            ),
            "{USAGE}"
        );
    }
}
