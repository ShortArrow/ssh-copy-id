//! The `ssh-copy-id` command.

use ssh_copy_id::app::{self, Environment};
use ssh_copy_id::cli_args::{self, ArgsError};
use ssh_copy_id::platform;
use ssh_copy_id::ssh_process::SystemSsh;
use std::io::{self, Write};
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
            if let Some(message) = describe(&error) {
                eprintln!("ssh-copy-id: ERROR: {message}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(1);
        }
    };
    platform::outlive_interrupts();
    let read_file = |path: &std::path::Path| std::fs::read(path);
    let exists = |path: &std::path::Path| path.exists();
    let env = Environment {
        has_console: platform::has_console(),
        askpass_set: std::env::var_os("SSH_ASKPASS").is_some(),
        home: platform::home_dir(),
        read_file: &read_file,
        exists: &exists,
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
fn describe(error: &ArgsError) -> Option<String> {
    Some(match error {
        ArgsError::Help => return None,
        ArgsError::NoDestination => "no destination given".to_string(),
        ArgsError::TooManyArguments(extra) => format!("Too many arguments: {}", extra.join(" ")),
        ArgsError::MissingValue(flag) => format!("option -{flag} requires a value"),
        ArgsError::MissingIdentity | ArgsError::IdentityWithoutFile => {
            "-i with a key file is required in this release".to_string()
        }
        ArgsError::Unsupported(flag) => format!("option -{flag} is not available in this release"),
        ArgsError::Unknown(option) => format!("unknown option {option}"),
    })
}
