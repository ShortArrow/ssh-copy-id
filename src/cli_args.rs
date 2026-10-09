//! Command-line parsing for the stage 1.5 subset of the upstream options.

use std::path::PathBuf;

/// An option passed through to `ssh`, in the order the user gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshOption {
    /// `-o option`
    Option(String),
    /// `-F config`
    Config(String),
}

/// Which keys the arguments select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySelection {
    /// `-i file`.
    File {
        /// The `-i` argument, with `.pub` added when absent.
        public_key: PathBuf,
        /// The public key file without `.pub`.
        private_key: PathBuf,
    },
    /// `-i` without a file: the default key file, as upstream's
    /// `use_id_file "${OPTARG:-$DEFAULT_PUB_ID_FILE}"`.
    DefaultFile,
    /// No `-i`: the agent's keys, otherwise the default key file.
    Unspecified,
}

/// A parsed invocation: one destination and the selected keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// `[user@]host`, passed to `ssh` unchanged.
    pub destination: String,
    /// The keys `-i` selects, or `Unspecified` without `-i`.
    pub key: KeySelection,
    /// `-p port`, passed to `ssh` unchanged.
    pub port: Option<String>,
    /// `-o` and `-F` options in their original order.
    pub ssh_options: Vec<SshOption>,
    /// `-f` and where it stands relative to `-i`.
    pub force: Force,
    /// `-n`: the keys that would be installed are listed instead of installed.
    pub dry_run: bool,
    /// `-t target_path`, as given; the last one counts, as in upstream.
    pub target: Option<String>,
    /// `-x`: each client command is printed before it runs, and the
    /// installation script traces itself with `set -x`.
    pub trace: bool,
}

/// Whether `-f` was given and whether it preceded the key selection. Upstream
/// selects the key file of `-i` when `getopts` reaches `-i`, requiring its
/// private key unless `-f` has been seen; without `-i` it selects after all
/// options. Either way, `-f` skips the installed-key check and the verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    /// No `-f`.
    Off,
    /// `-f` before `-i`, or without `-i`: the private key file is not required,
    /// and the login hint names `-i` without it, as upstream's unset `PRIV_ID_FILE`.
    BeforeKeySelection,
    /// `-f` only after `-i`: the private key file is required and the login
    /// hint names it, as without `-f`.
    AfterIdentity,
}

impl Force {
    /// Whether `-f` was given.
    pub fn is_on(self) -> bool {
        self != Force::Off
    }
}

/// Why the arguments do not form an invocation. Every variant exits 1, as upstream's usage does.
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
    /// `-i` was followed only by the last argument, and that argument names a
    /// key file, so upstream takes it for a forgotten destination. Holds the argument.
    MissingHostname(String),
    /// `-i` was given more than once.
    RepeatedIdentity,
    /// An upstream option that this release does not implement yet.
    Unsupported(char),
    /// An option letter upstream's `getopts` does not accept.
    IllegalOption(char),
    /// A long option, which upstream does not have.
    Unknown(String),
}

/// Parses the arguments after the program name.
///
/// Options follow upstream's `getopts` rules: they end at `--` or at the first
/// argument that does not start with `-`, flags may be grouped, and `-o`, `-F`,
/// `-p` and `-t` take their value attached or as the next argument. `-i` takes the
/// next argument as its file unless it looks like an option, as upstream does; when
/// that argument is the last one, it is the destination and `-i` has no file,
/// unless `names_a_key_file` holds for it, which is upstream's missing hostname.
pub fn parse(
    args: &[String],
    names_a_key_file: &dyn Fn(&str) -> bool,
) -> Result<Invocation, ArgsError> {
    let mut key = None;
    let mut port = None;
    let mut ssh_options = Vec::new();
    let mut force = Force::Off;
    let mut dry_run = false;
    let mut target = None;
    let mut trace = false;
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
                'o' | 'F' | 'p' | 't' => {
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
                        't' => target = Some(value),
                        _ => port = Some(value),
                    }
                    break;
                }
                'i' if key.is_some() => return Err(ArgsError::RepeatedIdentity),
                'i' => {
                    key = Some(match &args[index..] {
                        [last] if names_a_key_file(last) => {
                            return Err(ArgsError::MissingHostname(last.clone()));
                        }
                        [file, _, ..] if !looks_like_option(file) => {
                            index += 1;
                            selected_file(file)
                        }
                        _ => KeySelection::DefaultFile,
                    })
                }
                'h' | '?' => return Err(ArgsError::Help),
                'f' if force == Force::Off && key.is_some() => force = Force::AfterIdentity,
                'f' if force == Force::Off => force = Force::BeforeKeySelection,
                'f' => {}
                'n' => dry_run = true,
                'x' => trace = true,
                's' => return Err(ArgsError::Unsupported(flag)),
                _ => return Err(ArgsError::IllegalOption(flag)),
            }
        }
    }
    let mut rest = args[index.min(args.len())..].iter();
    let destination = rest.next().ok_or(ArgsError::NoDestination)?.clone();
    let extra: Vec<String> = rest.cloned().collect();
    if !extra.is_empty() {
        return Err(ArgsError::TooManyArguments(extra));
    }
    Ok(Invocation {
        destination,
        key: key.unwrap_or(KeySelection::Unspecified),
        port,
        ssh_options,
        force,
        dry_run,
        target,
        trace,
    })
}

/// The files `-i file` names: the public key file, with `.pub` added when absent, and the private key file without it.
fn selected_file(identity: &str) -> KeySelection {
    let private_key = PathBuf::from(identity.strip_suffix(".pub").unwrap_or(identity));
    let public_key = PathBuf::from(format!("{}.pub", private_key.display()));
    KeySelection::File {
        public_key,
        private_key,
    }
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

    fn parse_plain(given: &[String]) -> Result<Invocation, ArgsError> {
        parse(given, &|_| false)
    }

    fn selecting(destination: &str, key: KeySelection) -> Invocation {
        Invocation {
            destination: destination.to_string(),
            key,
            port: None,
            ssh_options: Vec::new(),
            force: Force::Off,
            dry_run: false,
            target: None,
            trace: false,
        }
    }

    fn invocation(destination: &str, key: &str) -> Invocation {
        selecting(
            destination,
            KeySelection::File {
                public_key: PathBuf::from(format!("{key}.pub")),
                private_key: PathBuf::from(key),
            },
        )
    }

    #[test]
    fn p01_identity_without_pub_suffix_gets_it() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "host"])),
            Ok(invocation("host", "k"))
        );
    }

    #[test]
    fn p02_identity_with_pub_suffix_keeps_it() {
        assert_eq!(
            parse_plain(&args(&["-i", "k.pub", "user@host"])),
            Ok(invocation("user@host", "k"))
        );
    }

    #[test]
    fn p03_port_as_separate_argument() {
        let parsed = parse_plain(&args(&["-p", "2222", "-i", "k", "h"])).unwrap();
        assert_eq!(parsed.port.as_deref(), Some("2222"));
    }

    #[test]
    fn p04_port_attached() {
        let parsed = parse_plain(&args(&["-p2222", "-i", "k", "h"])).unwrap();
        assert_eq!(parsed.port.as_deref(), Some("2222"));
    }

    #[test]
    fn p05_o_and_f_keep_their_order() {
        let parsed = parse_plain(&args(&[
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
        let parsed = parse_plain(&args(&["-oA=1", "-Fcfg", "-i", "k", "h"])).unwrap();
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
        assert_eq!(
            parse_plain(&args(&["-p", "22"])),
            Err(ArgsError::NoDestination)
        );
        assert_eq!(parse_plain(&args(&[])), Err(ArgsError::NoDestination));
    }

    #[test]
    fn p08_too_many_arguments() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "a", "b"])),
            Err(ArgsError::TooManyArguments(args(&["b"])))
        );
    }

    #[test]
    fn p09_value_missing_at_the_end() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "-p"])),
            Err(ArgsError::MissingValue('p'))
        );
        assert_eq!(
            parse_plain(&args(&["-o"])),
            Err(ArgsError::MissingValue('o'))
        );
    }

    #[test]
    fn p10_help() {
        assert_eq!(parse_plain(&args(&["-h"])), Err(ArgsError::Help));
        assert_eq!(parse_plain(&args(&["-?"])), Err(ArgsError::Help));
    }

    #[test]
    fn p11_later_stage_options_are_unsupported() {
        assert_eq!(
            parse_plain(&args(&["-s", "-i", "k", "h"])),
            Err(ArgsError::Unsupported('s'))
        );
    }

    #[test]
    fn p12_unknown_option() {
        assert_eq!(
            parse_plain(&args(&["-z", "-i", "k", "h"])),
            Err(ArgsError::IllegalOption('z'))
        );
    }

    #[test]
    fn p13_double_dash_ends_options() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "--", "host"])),
            Ok(invocation("host", "k"))
        );
    }

    #[test]
    fn p14_identity_before_an_option_selects_the_default_file() {
        let mut expected = selecting("h", KeySelection::DefaultFile);
        expected.port = Some("22".into());
        assert_eq!(parse_plain(&args(&["-i", "-p", "22", "h"])), Ok(expected));
    }

    #[test]
    fn p15_options_stop_at_the_destination() {
        assert_eq!(
            parse_plain(&args(&["host", "-i", "k"])),
            Err(ArgsError::TooManyArguments(args(&["-i", "k"])))
        );
    }

    #[test]
    fn p16_without_identity_the_key_is_unspecified() {
        assert_eq!(
            parse_plain(&args(&["host"])),
            Ok(selecting("host", KeySelection::Unspecified))
        );
    }

    #[test]
    fn p17_grouped_flags_report_the_first_unsupported() {
        assert_eq!(
            parse_plain(&args(&["-fs", "-i", "k", "h"])),
            Err(ArgsError::Unsupported('s'))
        );
    }

    #[test]
    fn p18_repeated_identity_is_an_error() {
        assert_eq!(
            parse_plain(&args(&["-i", "a", "-i", "b", "h"])),
            Err(ArgsError::RepeatedIdentity)
        );
    }

    #[test]
    fn p19_identity_followed_only_by_the_destination_selects_the_default_file() {
        assert_eq!(
            parse_plain(&args(&["-i", "host"])),
            Ok(selecting("host", KeySelection::DefaultFile))
        );
        let mut expected = selecting("k.pub", KeySelection::DefaultFile);
        expected.port = Some("22".into());
        assert_eq!(
            parse_plain(&args(&["-p", "22", "-i", "k.pub"])),
            Ok(expected)
        );
    }

    #[test]
    fn p20_identity_takes_a_dash_argument_that_is_not_an_option_letter() {
        assert_eq!(
            parse_plain(&args(&["-i", "-mykey", "host"])),
            Ok(invocation("host", "-mykey"))
        );
        assert_eq!(
            parse_plain(&args(&["-i", "-", "host"])),
            Ok(invocation("host", "-"))
        );
    }

    #[test]
    fn p21_identity_has_no_file_before_an_upstream_option_letter() {
        for letter in ['i', 'o', 'p', 'F', 't', 'f', 'n', 's', 'x', 'h', '?', '-'] {
            let parsed = parse_plain(&args(&["-i", &format!("-{letter}k"), "h"]));
            assert!(
                !matches!(
                    parsed,
                    Ok(Invocation {
                        key: KeySelection::File { .. },
                        ..
                    })
                ),
                "letter {letter}: {parsed:?}"
            );
        }
        let mut expected = selecting("h", KeySelection::DefaultFile);
        expected.ssh_options = vec![SshOption::Option("k".into())];
        assert_eq!(parse_plain(&args(&["-i", "-ok", "h"])), Ok(expected));
    }

    #[test]
    fn p22_unknown_long_option_is_named_whole() {
        assert_eq!(
            parse_plain(&args(&["--target-os", "unix", "h"])),
            Err(ArgsError::Unknown("--target-os".into()))
        );
    }

    #[test]
    fn p23_a_last_argument_naming_a_key_file_is_a_missing_hostname() {
        let names_a_key_file = |argument: &str| {
            assert_eq!(argument, "k.pub");
            true
        };
        assert_eq!(
            parse(&args(&["-i", "k.pub"]), &names_a_key_file),
            Err(ArgsError::MissingHostname("k.pub".into()))
        );
    }

    #[test]
    fn p24_double_dash_after_identity_makes_a_key_file_the_destination() {
        assert_eq!(
            parse(&args(&["-i", "--", "k.pub"]), &|_| true),
            Ok(selecting("k.pub", KeySelection::DefaultFile))
        );
    }

    #[test]
    fn p25_identity_as_the_only_argument_leaves_no_destination() {
        assert_eq!(parse_plain(&args(&["-i"])), Err(ArgsError::NoDestination));
    }

    #[test]
    fn p26_force_is_off_unless_given() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "h"])).unwrap().force,
            Force::Off
        );
    }

    #[test]
    fn p27_force_before_identity_or_without_it_precedes_the_key_selection() {
        for given in [
            args(&["-f", "-i", "k", "h"]),
            args(&["-fi", "k", "h"]),
            args(&["-f", "h"]),
            args(&["-f", "-i", "h"]),
            args(&["-f", "-i", "k", "-f", "h"]),
        ] {
            let parsed = parse_plain(&given).unwrap();
            assert_eq!(parsed.force, Force::BeforeKeySelection, "{given:?}");
        }
    }

    #[test]
    fn p28_force_only_after_identity_follows_the_key_selection() {
        for given in [
            args(&["-i", "k", "-f", "h"]),
            args(&["-i", "-f", "h"]),
            args(&["-i", "k", "-f", "-f", "h"]),
        ] {
            let parsed = parse_plain(&given).unwrap();
            assert_eq!(parsed.force, Force::AfterIdentity, "{given:?}");
        }
        assert_eq!(
            parse_plain(&args(&["-i", "k", "-f", "h"])).unwrap().key,
            invocation("h", "k").key
        );
    }

    #[test]
    fn p29_dry_run_is_off_unless_given() {
        assert!(!parse_plain(&args(&["-i", "k", "h"])).unwrap().dry_run);
    }

    #[test]
    fn p30_dry_run_alone_or_grouped_leaves_the_rest_unchanged() {
        assert_eq!(
            parse_plain(&args(&["-n", "-i", "k", "h"])),
            Ok(Invocation {
                dry_run: true,
                ..invocation("h", "k")
            })
        );
        let grouped = parse_plain(&args(&["-fn", "-i", "k", "h"])).unwrap();
        assert!(grouped.dry_run);
        assert_eq!(grouped.force, Force::BeforeKeySelection);
    }

    #[test]
    fn p31_target_is_none_unless_given() {
        assert_eq!(parse_plain(&args(&["-i", "k", "h"])).unwrap().target, None);
    }

    #[test]
    fn p32_target_as_separate_or_attached_argument_is_taken_as_given() {
        for (given, expected) in [
            (
                args(&["-t", "keys/my file", "-i", "k", "h"]),
                "keys/my file",
            ),
            (args(&["-tkeys/x", "-i", "k", "h"]), "keys/x"),
            (args(&["-ft", "-n", "-i", "k", "h"]), "-n"),
            (args(&["-t", "a\nb", "h"]), "a\nb"),
        ] {
            let parsed = parse_plain(&given).unwrap();
            assert_eq!(parsed.target.as_deref(), Some(expected), "{given:?}");
        }
    }

    #[test]
    fn p33_target_without_a_value_is_reported_as_getopts_does() {
        assert_eq!(
            parse_plain(&args(&["-i", "k", "-t"])),
            Err(ArgsError::MissingValue('t'))
        );
    }

    #[test]
    fn p34_a_later_target_replaces_an_earlier_one_as_upstream() {
        let parsed = parse_plain(&args(&["-t", "a", "-t", "b", "h"])).unwrap();
        assert_eq!(parsed.target.as_deref(), Some("b"));
    }

    #[test]
    fn p35_trace_is_off_unless_given() {
        assert!(!parse_plain(&args(&["-i", "k", "h"])).unwrap().trace);
    }

    #[test]
    fn p36_trace_alone_or_grouped_leaves_the_rest_unchanged() {
        assert_eq!(
            parse_plain(&args(&["-x", "-i", "k", "h"])),
            Ok(Invocation {
                trace: true,
                ..invocation("h", "k")
            })
        );
        let grouped = parse_plain(&args(&["-fnx", "-t", "a", "h"])).unwrap();
        assert!(grouped.trace);
        assert!(grouped.dry_run);
        assert_eq!(grouped.force, Force::BeforeKeySelection);
        assert_eq!(grouped.target.as_deref(), Some("a"));
    }
}
