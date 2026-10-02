//! The `ssh-copy-id` command.

use ssh_copy_id::app::{self, Environment};
use ssh_copy_id::cli_args::{self, ArgsError};
use ssh_copy_id::platform;
use ssh_copy_id::ssh_process::SystemSsh;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage: ssh-copy-id [-h|-?] -i identity_file [-p port] [-F ssh_config] [[-o ssh_option] ...] [user@]hostname
\t-i: the public key to install; '.pub' is added when absent
\t-p: port of the remote host
\t-F, -o: passed to ssh unchanged
\t-h|-?: print this help
This release installs one explicitly selected key on a Unix-like host.
-f, -n, -s, -t, -x, and -i without a file are not available yet.";

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
    let invocation = match cli_args::parse(&args) {
        Ok(invocation) => invocation,
        Err(error) => {
            if let Some(message) = describe(&error, &|path| std::fs::read(path)) {
                eprintln!("ssh-copy-id: ERROR: {message}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(1);
        }
    };
    platform::outlive_interrupts();
    let read_file = |path: &std::path::Path| std::fs::read(path);
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
        readable_file: &platform::is_readable_file,
        same_file: &same_file,
        create_scratch_dir: &platform::create_scratch_dir,
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

/// The message printed above the usage, or `None` for `-h` and `-?`.
///
/// `read_file` reads the argument that followed a file-less `-i`, to tell upstream's missing hostname case apart.
fn describe(error: &ArgsError, read_file: &dyn Fn(&Path) -> io::Result<Vec<u8>>) -> Option<String> {
    Some(match error {
        ArgsError::Help => return None,
        ArgsError::NoDestination => "no destination given".to_string(),
        ArgsError::TooManyArguments(extra) => format!("Too many arguments: {}", extra.join(" ")),
        ArgsError::MissingValue(flag) => format!("option -{flag} requires a value"),
        ArgsError::IdentityBeforeDestinationOnly(argument)
            if names_a_key_file(argument, read_file) =>
        {
            "Missing hostname".to_string()
        }
        ArgsError::MissingIdentity
        | ArgsError::IdentityWithoutFile
        | ArgsError::IdentityBeforeDestinationOnly(_) => {
            "-i with a key file is required in this release".to_string()
        }
        ArgsError::RepeatedIdentity => "-i option must not be specified more than once".to_string(),
        ArgsError::Unsupported(flag) => format!("option -{flag} is not available in this release"),
        ArgsError::Unknown(option) => format!("unknown option {option}"),
    })
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

    const STAGE_1_MESSAGE: &str = "-i with a key file is required in this release";

    fn describe_last(
        argument: &str,
        read: &dyn Fn(&Path) -> io::Result<Vec<u8>>,
    ) -> Option<String> {
        describe(
            &ArgsError::IdentityBeforeDestinationOnly(argument.to_string()),
            read,
        )
    }

    #[test]
    fn m01_readable_file_containing_ssh_is_a_missing_hostname() {
        let read = |path: &Path| {
            assert_eq!(path, Path::new("k.pub"));
            Ok(b"ssh-ed25519 AAAA user@laptop
"
            .to_vec())
        };
        assert_eq!(
            describe_last("k.pub", &read).as_deref(),
            Some("Missing hostname")
        );
    }

    #[test]
    fn m02_ssh_is_matched_without_case() {
        let read = |_: &Path| Ok(b"key from SSH agent".to_vec());
        assert_eq!(
            describe_last("k", &read).as_deref(),
            Some("Missing hostname")
        );
    }

    #[test]
    fn m03_file_without_ssh_is_the_stage_1_error() {
        let read = |_: &Path| Ok(b"not a key".to_vec());
        assert_eq!(
            describe_last("host", &read).as_deref(),
            Some(STAGE_1_MESSAGE)
        );
    }

    #[test]
    fn m04_unreadable_argument_is_the_stage_1_error() {
        let read = |_: &Path| Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(
            describe_last("host", &read).as_deref(),
            Some(STAGE_1_MESSAGE)
        );
    }

    #[test]
    fn m05_unknown_long_option_is_named() {
        let read = |_: &Path| -> io::Result<Vec<u8>> { unreachable!() };
        assert_eq!(
            describe(&ArgsError::Unknown("--target-os".into()), &read).as_deref(),
            Some("unknown option --target-os")
        );
    }
}
