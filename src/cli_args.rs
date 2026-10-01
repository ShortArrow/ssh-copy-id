//! Command-line parsing for the stage 1 subset of the upstream options.

use std::path::PathBuf;

/// An option passed through to `ssh`, in the order the user gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshOption {
    /// `-o option`
    Option(String),
    /// `-F config`
    Config(String),
}

/// A parsed invocation: one destination and one explicitly selected key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// `[user@]host`, passed to `ssh` unchanged.
    pub destination: String,
    /// The public key file: the `-i` argument, with `.pub` added when absent.
    pub public_key: PathBuf,
    /// The private key file: the public key file without `.pub`.
    pub private_key: PathBuf,
    /// `-p port`, passed to `ssh` unchanged.
    pub port: Option<String>,
    /// `-o` and `-F` options in their original order.
    pub ssh_options: Vec<SshOption>,
}

/// Why the arguments do not form a stage 1 invocation. Every variant exits 1, as upstream's usage does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgsError {
    /// `-h` or `-?`.
    Help,
    /// No destination was given.
    NoDestination,
    /// Arguments remain after the destination.
    TooManyArguments(Vec<String>),
    /// An option that takes a value was last.
    MissingValue(char),
    /// No `-i` was given; default key selection is not available yet.
    MissingIdentity,
    /// `-i` was not followed by a file name; default key selection is not available yet.
    IdentityWithoutFile,
    /// `-i` was followed only by the last argument, which upstream takes as the destination, so `-i` has no file.
    /// Holds that argument: upstream reports a missing hostname when it is a readable file containing `ssh`.
    IdentityBeforeDestinationOnly(String),
    /// `-i` was given more than once.
    RepeatedIdentity,
    /// An upstream option that this release does not implement yet.
    Unsupported(char),
    /// An option upstream does not have.
    Unknown(String),
}

/// Parses the arguments after the program name.
///
/// Options follow upstream's `getopts` rules: they end at `--` or at the first
/// argument that does not start with `-`, flags may be grouped, and `-o`, `-F`
/// and `-p` take their value attached or as the next argument. `-i` takes the
/// next argument as its file unless it looks like an option, as upstream does; when
/// that argument is the last one, it is the destination and `-i` has no file.
pub fn parse(args: &[String]) -> Result<Invocation, ArgsError> {
    let mut identity: Option<String> = None;
    let mut port = None;
    let mut ssh_options = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if arg == "--" {
            break;
        }
        if arg.starts_with("--") {
            return Err(ArgsError::Unknown(arg.clone()));
        }
        let Some(flags) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            index -= 1;
            break;
        };
        for (at, flag) in flags.char_indices() {
            match flag {
                'o' | 'F' | 'p' => {
                    let attached = &flags[at + flag.len_utf8()..];
                    let value = if attached.is_empty() {
                        index += 1;
                        args.get(index - 1)
                            .cloned()
                            .ok_or(ArgsError::MissingValue(flag))?
                    } else {
                        attached.to_string()
                    };
                    match flag {
                        'o' => ssh_options.push(SshOption::Option(value)),
                        'F' => ssh_options.push(SshOption::Config(value)),
                        _ => port = Some(value),
                    }
                    break;
                }
                'i' if identity.is_some() => return Err(ArgsError::RepeatedIdentity),
                'i' => match &args[index..] {
                    [last] => return Err(ArgsError::IdentityBeforeDestinationOnly(last.clone())),
                    [file, ..] if !looks_like_option(file) => {
                        identity = Some(file.clone());
                        index += 1;
                    }
                    _ => return Err(ArgsError::IdentityWithoutFile),
                },
                'h' | '?' => return Err(ArgsError::Help),
                'f' | 'n' | 's' | 't' | 'x' => return Err(ArgsError::Unsupported(flag)),
                _ => return Err(ArgsError::Unknown(format!("-{flag}"))),
            }
        }
    }
    let mut rest = args[index.min(args.len())..].iter();
    let destination = rest.next().ok_or(ArgsError::NoDestination)?.clone();
    let extra: Vec<String> = rest.cloned().collect();
    if !extra.is_empty() {
        return Err(ArgsError::TooManyArguments(extra));
    }
    let identity = identity.ok_or(ArgsError::MissingIdentity)?;
    let private_key = PathBuf::from(identity.strip_suffix(".pub").unwrap_or(&identity));
    let public_key = PathBuf::from(format!("{}.pub", private_key.display()));
    Ok(Invocation {
        destination,
        public_key,
        private_key,
        port,
        ssh_options,
    })
}

/// Upstream's test for an argument that `-i` must not take as its file: `-` followed by
/// one of upstream's option letters or `-`.
fn looks_like_option(arg: &str) -> bool {
    let mut chars = arg.chars();
    chars.next() == Some('-')
        && chars
            .next()
            .is_some_and(|letter| "iopFtfnsxh?-".contains(letter))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn invocation(destination: &str, key: &str) -> Invocation {
        Invocation {
            destination: destination.to_string(),
            public_key: PathBuf::from(format!("{key}.pub")),
            private_key: PathBuf::from(key),
            port: None,
            ssh_options: Vec::new(),
        }
    }

    #[test]
    fn p01_identity_without_pub_suffix_gets_it() {
        assert_eq!(
            parse(&args(&["-i", "k", "host"])),
            Ok(invocation("host", "k"))
        );
    }

    #[test]
    fn p02_identity_with_pub_suffix_keeps_it() {
        assert_eq!(
            parse(&args(&["-i", "k.pub", "user@host"])),
            Ok(invocation("user@host", "k"))
        );
    }

    #[test]
    fn p03_port_as_separate_argument() {
        let parsed = parse(&args(&["-p", "2222", "-i", "k", "h"])).unwrap();
        assert_eq!(parsed.port.as_deref(), Some("2222"));
    }

    #[test]
    fn p04_port_attached() {
        let parsed = parse(&args(&["-p2222", "-i", "k", "h"])).unwrap();
        assert_eq!(parsed.port.as_deref(), Some("2222"));
    }

    #[test]
    fn p05_o_and_f_keep_their_order() {
        let parsed = parse(&args(&[
            "-o", "A=1", "-F", "cfg", "-o", "B=2", "-i", "k", "h",
        ]))
        .unwrap();
        assert_eq!(
            parsed.ssh_options,
            vec![
                SshOption::Option("A=1".into()),
                SshOption::Config("cfg".into()),
                SshOption::Option("B=2".into()),
            ]
        );
    }

    #[test]
    fn p06_option_value_attached() {
        let parsed = parse(&args(&["-oA=1", "-Fcfg", "-i", "k", "h"])).unwrap();
        assert_eq!(
            parsed.ssh_options,
            vec![
                SshOption::Option("A=1".into()),
                SshOption::Config("cfg".into())
            ]
        );
    }

    #[test]
    fn p07_no_destination() {
        assert_eq!(parse(&args(&["-p", "22"])), Err(ArgsError::NoDestination));
        assert_eq!(parse(&args(&[])), Err(ArgsError::NoDestination));
    }

    #[test]
    fn p08_too_many_arguments() {
        assert_eq!(
            parse(&args(&["-i", "k", "a", "b"])),
            Err(ArgsError::TooManyArguments(args(&["b"])))
        );
    }

    #[test]
    fn p09_value_missing_at_the_end() {
        assert_eq!(
            parse(&args(&["-i", "k", "-p"])),
            Err(ArgsError::MissingValue('p'))
        );
        assert_eq!(parse(&args(&["-o"])), Err(ArgsError::MissingValue('o')));
    }

    #[test]
    fn p10_help() {
        assert_eq!(parse(&args(&["-h"])), Err(ArgsError::Help));
        assert_eq!(parse(&args(&["-?"])), Err(ArgsError::Help));
    }

    #[test]
    fn p11_later_stage_options_are_unsupported() {
        for flag in ['f', 'n', 's', 't', 'x'] {
            let given = args(&[&format!("-{flag}"), "-i", "k", "h"]);
            assert_eq!(
                parse(&given),
                Err(ArgsError::Unsupported(flag)),
                "flag {flag}"
            );
        }
    }

    #[test]
    fn p12_unknown_option() {
        assert_eq!(
            parse(&args(&["-z", "-i", "k", "h"])),
            Err(ArgsError::Unknown("-z".into()))
        );
    }

    #[test]
    fn p13_double_dash_ends_options() {
        assert_eq!(
            parse(&args(&["-i", "k", "--", "host"])),
            Ok(invocation("host", "k"))
        );
    }

    #[test]
    fn p14_identity_without_file() {
        assert_eq!(
            parse(&args(&["-i", "-p", "22", "h"])),
            Err(ArgsError::IdentityWithoutFile)
        );
        assert_eq!(parse(&args(&["-i"])), Err(ArgsError::IdentityWithoutFile));
    }

    #[test]
    fn p15_options_stop_at_the_destination() {
        assert_eq!(
            parse(&args(&["host", "-i", "k"])),
            Err(ArgsError::TooManyArguments(args(&["-i", "k"])))
        );
    }

    #[test]
    fn p16_identity_is_required_in_stage_1() {
        assert_eq!(parse(&args(&["host"])), Err(ArgsError::MissingIdentity));
    }

    #[test]
    fn p17_grouped_flags_report_the_first_unsupported() {
        assert_eq!(
            parse(&args(&["-fn", "-i", "k", "h"])),
            Err(ArgsError::Unsupported('f'))
        );
    }

    #[test]
    fn p18_repeated_identity_is_an_error() {
        assert_eq!(
            parse(&args(&["-i", "a", "-i", "b", "h"])),
            Err(ArgsError::RepeatedIdentity)
        );
    }

    #[test]
    fn p19_identity_followed_only_by_the_destination_has_no_file() {
        assert_eq!(
            parse(&args(&["-i", "host"])),
            Err(ArgsError::IdentityBeforeDestinationOnly("host".into()))
        );
        assert_eq!(
            parse(&args(&["-p", "22", "-i", "k.pub"])),
            Err(ArgsError::IdentityBeforeDestinationOnly("k.pub".into()))
        );
    }

    #[test]
    fn p20_identity_takes_a_dash_argument_that_is_not_an_option_letter() {
        assert_eq!(
            parse(&args(&["-i", "-mykey", "host"])),
            Ok(invocation("host", "-mykey"))
        );
        assert_eq!(
            parse(&args(&["-i", "-", "host"])),
            Ok(invocation("host", "-"))
        );
    }

    #[test]
    fn p21_identity_has_no_file_before_an_upstream_option_letter() {
        for letter in ['i', 'o', 'p', 'F', 't', 'f', 'n', 's', 'x', 'h', '?', '-'] {
            assert_eq!(
                parse(&args(&["-i", &format!("-{letter}k"), "h", "extra"])),
                Err(ArgsError::IdentityWithoutFile),
                "letter {letter}"
            );
        }
    }

    #[test]
    fn p22_unknown_long_option_is_named_whole() {
        assert_eq!(
            parse(&args(&["--target-os", "unix", "h"])),
            Err(ArgsError::Unknown("--target-os".into()))
        );
    }
}
