//! One run of the CLI: from the selected keys to the reported outcome.

use crate::cli_args::{Invocation, KeySelection, SshOption};
use crate::default_key::{DirEntryTime, newest_public_key};
use crate::installed_check::{
    CheckResult, classify, is_certificate, is_tested_client, other_candidates_matching,
};
use crate::key_input::{InputError, prepare};
use crate::remote_script::{install_command, sh_quote};
use crate::result_line::{Outcome, parse_report};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The key file on Unix-like destinations, relative to the home directory.
pub const TARGET: &str = ".ssh/authorized_keys";

const NOTHING_WRITTEN: &str = "interrupted; nothing was written";
const CERTIFICATE_REASON: &str =
    "the key is a certificate, which authorized_keys does not authenticate";

/// What one `ssh` run returned. `status` is `None` when the process was terminated by a signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs the system `ssh` client.
pub trait Ssh {
    /// Runs `ssh` with `args` and writes `stdin` to it. When `capture_stderr` is
    /// false, stderr goes to the user and `SshOutput::stderr` is empty.
    fn run(&mut self, args: &[String], stdin: &[u8], capture_stderr: bool)
    -> io::Result<SshOutput>;

    /// Runs `ssh-add -L`, which lists the agent's public keys, with stdout and stderr captured.
    fn list_agent_keys(&mut self) -> io::Result<SshOutput>;
}

/// The parts of the environment a run depends on.
pub struct Environment<'a> {
    /// Whether the process has a console or terminal on which `ssh` can prompt.
    pub has_console: bool,
    /// Whether `SSH_ASKPASS` is set.
    pub askpass_set: bool,
    /// The home directory used to expand a leading `~/` or `~\` in the `-i`
    /// path and `~/` in `ssh -G` output.
    pub home: PathBuf,
    pub read_file: &'a dyn Fn(&Path) -> io::Result<Vec<u8>>,
    pub exists: &'a dyn Fn(&Path) -> bool,
    /// Opens a path for reading and succeeds when it is a regular file; otherwise
    /// returns why it cannot be read.
    pub readable_file: &'a dyn Fn(&Path) -> io::Result<()>,
    /// Whether two paths name the same file.
    pub same_file: &'a dyn Fn(&Path, &Path) -> bool,
    /// The name and modification time of every entry in a directory, as `ls -d` sees them.
    pub modification_times: &'a dyn Fn(&Path) -> io::Result<Vec<DirEntryTime>>,
    /// Creates a new directory, readable only by its owner, inside the given
    /// existing directory and returns its path; the probes' `ssh -E` logs go there.
    pub create_scratch_dir: &'a dyn Fn(&Path) -> io::Result<PathBuf>,
    /// Creates or replaces a file with the given contents; an agent key's file goes in the scratch directory.
    pub write_file: &'a dyn Fn(&Path, &[u8]) -> io::Result<()>,
    /// Removes a directory and its contents; a failure is ignored.
    pub remove_dir: &'a dyn Fn(&Path),
    /// Whether a console interrupt has arrived during the run.
    pub interrupted: &'a dyn Fn() -> bool,
}

/// A directory created for one run, removed when the value is dropped.
struct ScratchDir<'a> {
    path: PathBuf,
    remove: &'a dyn Fn(&Path),
}

impl Drop for ScratchDir<'_> {
    fn drop(&mut self) {
        (self.remove)(&self.path);
    }
}

/// Runs the flow and returns the exit status: 0 when the keys are installed
/// or were already installed, 1 otherwise. Diagnostics go to `err`, and the final
/// summary to `out`, as upstream prints them.
///
/// The probes' logs live in a scratch directory under `<home>/.ssh`, created
/// before the first `ssh` run and removed on every return after it. After each
/// `ssh` run, an interrupt stops the run with exit status 1: before the
/// installation nothing was written; after it the parsed outcome is reported and
/// verification is not attempted or not reported.
pub fn run(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match install(invocation, env, ssh, out, err) {
        Ok(()) => 0,
        Err(Stop::Error(message)) => {
            let _ = writeln!(err, "ssh-copy-id: ERROR: {message}");
            1
        }
        Err(Stop::ErrorAfterBlankLine(message)) => {
            let _ = writeln!(err, "\nssh-copy-id: ERROR: {message}");
            1
        }
        Err(Stop::Relayed(lines)) => {
            let relayed: Vec<String> = lines.iter().map(|line| format!("ERROR: {line}")).collect();
            let _ = write!(err, "\nssh-copy-id: {}\n\n", relayed.join("\n"));
            1
        }
    }
}

/// Why a run stopped, each printed to stderr in upstream's form for it.
enum Stop {
    /// `ssh-copy-id: ERROR: <message>`.
    Error(String),
    /// The same line after a blank line, as upstream reports a key file it cannot open.
    ErrorAfterBlankLine(String),
    /// `ssh`'s own lines, as upstream relays a failed probe: each prefixed with
    /// `ERROR: `, the first also with `ssh-copy-id: `, between blank lines.
    Relayed(Vec<String>),
}

impl From<String> for Stop {
    fn from(message: String) -> Stop {
        Stop::Error(message)
    }
}

fn stop<T>(message: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Error(message.into()))
}

/// The keys a run installs, selected before any connection.
enum Selection {
    /// A key file's lines, holding one key, checked with its private key.
    File {
        text: Vec<u8>,
        /// The private key file, the `-i` argument of the check.
        identity: String,
        /// Whether `-i` was given, so that the login hint names the private
        /// key, as upstream's `${SEEN_OPT_I:+-i …}`.
        named_by_option: bool,
    },
    /// The key lines `ssh-add -L` listed, in order.
    Agent(Vec<Vec<u8>>),
}

/// One key ready for its checks.
struct SelectedKey {
    /// Its position among the key lines of its source, from 1.
    number: usize,
    /// The lines sent when it is installed, each ending in LF.
    text: Vec<u8>,
    /// The `-i` argument of its installed-key check: the private key file, or
    /// for an agent key the one-line public key file written for it.
    identity: String,
    /// What precedes a message about it: empty unless its source holds several keys.
    label: String,
}

const AGENT_SOURCE: &str = "ssh-add -L";

/// What the login hint says about `-i`, as upstream's
/// `${SEEN_OPT_I:+-i${PRIV_ID_FILE:+ $PRIV_ID_FILE} }`.
enum HintIdentity<'a> {
    /// `-i` was not given: nothing.
    Absent,
    /// `-i` was given with `-f`, under which upstream leaves the private key
    /// unset: `-i` without a value.
    Bare,
    /// `-i` was given: `-i` and the private key file.
    Named(&'a str),
}

impl Selection {
    /// What the login hint says about `-i`: the private key when `-i` selected
    /// it, and `-i` alone when `-f` was also given.
    fn hint_identity(&self, force: bool) -> HintIdentity<'_> {
        match self {
            Selection::File {
                named_by_option: true,
                ..
            } if force => HintIdentity::Bare,
            Selection::File {
                identity,
                named_by_option: true,
                ..
            } => HintIdentity::Named(identity),
            _ => HintIdentity::Absent,
        }
    }

    /// The lines of every selected key, in order, as they are sent.
    fn texts(&self) -> Vec<Vec<u8>> {
        match self {
            Selection::File { text, .. } => vec![text.clone()],
            Selection::Agent(lines) => lines.clone(),
        }
    }

    /// The keys to check, each agent key written as `agent-key-<n>.pub` in `scratch`, as upstream's `L_TMP_ID_FILE`.
    fn keys(&self, scratch: &Path, env: &Environment) -> Result<Vec<SelectedKey>, String> {
        match self {
            Selection::File { text, identity, .. } => Ok(vec![SelectedKey {
                number: 1,
                text: text.clone(),
                identity: identity.clone(),
                label: String::new(),
            }]),
            Selection::Agent(lines) => lines
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    let number = index + 1;
                    let path = scratch.join(format!("agent-key-{number}.pub"));
                    (env.write_file)(&path, text)
                        .map_err(|e| format!("cannot write {}: {}", path.display(), reason(&e)))?;
                    Ok(SelectedKey {
                        number,
                        text: text.clone(),
                        identity: path.display().to_string(),
                        label: if lines.len() > 1 {
                            format!("key {number} from {AGENT_SOURCE}: ")
                        } else {
                            String::new()
                        },
                    })
                })
                .collect(),
        }
    }
}

fn install(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Stop> {
    let selection = select_keys(invocation, env, ssh, err)?;
    let scratch = ScratchDir {
        path: (env.create_scratch_dir)(&env.home.join(".ssh")).map_err(|_| {
            format!(
                "failed to create required temporary directory under ~/.ssh (HOME=\"{}\")",
                env.home.display()
            )
        })?,
        remove: env.remove_dir,
    };
    let common = common_args(invocation, env);
    if invocation.force {
        announce_batch_mode(env, err);
        return install_unchecked(invocation, env, ssh, out, &selection, &common);
    }
    let keys = selection.keys(&scratch.path, env)?;

    warn_about_an_untested_client(ssh, env, err)?;
    announce_batch_mode(env, err);
    let mut candidates = Vec::new();
    for key in &keys {
        let others = other_candidates(ssh, env, &common, &invocation.destination, &key.identity)?;
        candidates.push((key, others));
    }
    let probe = |ssh: &mut dyn Ssh, key: &SelectedKey, others: &[String], stage: &str| {
        if is_certificate(&key.text) {
            return Ok(CheckResult::Inconclusive(CERTIFICATE_REASON.to_string()));
        }
        let log = scratch.path.join(format!("{stage}-{}.log", key.number));
        let args = probe_args(&key.identity, &log, &common, &invocation.destination);
        check(ssh, env, &args, &log, others)
    };

    info(
        err,
        "attempting to log in with the new key(s), to filter out any that are already installed",
    );
    let mut remaining = Vec::new();
    for (key, others) in candidates {
        let checked = probe(ssh, key, &others, "check")?;
        stop_if_interrupted(env, NOTHING_WRITTEN)?;
        match checked {
            CheckResult::Installed => continue,
            CheckResult::NotAttempted { messages, .. } if !messages.is_empty() => {
                return Err(Stop::Relayed(messages));
            }
            CheckResult::Failed(message)
            | CheckResult::NotAttempted {
                failure: message, ..
            } => {
                return stop(message);
            }
            CheckResult::Inconclusive(reason) => warn(
                err,
                &format!(
                    "{}could not tell whether the key is already installed ({reason}); \
                     installing it, which may add a duplicate",
                    key.label
                ),
            ),
            CheckResult::NotInstalled => {}
        }
        remaining.push((key, others));
    }
    if remaining.is_empty() {
        let _ = write!(
            err,
            "\nssh-copy-id: WARNING: All keys were skipped because they already exist \
             on the remote system.\n\
             \t\t(if you think this is a mistake, you may want to use -f option)\n\n"
        );
        return Ok(());
    }

    info(
        err,
        &format!(
            "{} key(s) remain to be installed -- if you are prompted now it is to install the new keys",
            remaining.len()
        ),
    );
    let text: Vec<u8> = remaining
        .iter()
        .flat_map(|(key, _)| key.text.iter().copied())
        .collect();
    let Written { added, target } = write_keys(ssh, invocation, &common, &text, remaining.len())?;

    let login = login_command(invocation, selection.hint_identity(invocation.force));
    if (env.interrupted)() {
        summary(out, added, &login);
        return stop("interrupted".to_string());
    }
    for (key, others) in &remaining {
        let verified = probe(ssh, key, others, "verify")?;
        if (env.interrupted)() {
            summary(out, added, &login);
            return stop("interrupted before the key was verified".to_string());
        }
        report_verification(err, &key.label, verified, &target);
    }

    summary(out, added, &login);
    Ok(())
}

/// Installs every selected key without the installed-key check or the
/// verification, as `-f` asks; duplicates may result.
fn install_unchecked(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    out: &mut dyn Write,
    selection: &Selection,
    common: &[String],
) -> Result<(), Stop> {
    let texts = selection.texts();
    let Written { added, .. } = write_keys(ssh, invocation, common, &texts.concat(), texts.len())?;
    summary(
        out,
        added,
        &login_command(invocation, selection.hint_identity(invocation.force)),
    );
    Ok(stop_if_interrupted(env, "interrupted")?)
}

fn announce_batch_mode(env: &Environment, err: &mut dyn Write) {
    if batch_mode(env) {
        info(
            err,
            "there is no console and SSH_ASKPASS is not set, so ssh runs with BatchMode=yes; \
             password and passphrase prompts fail",
        );
    }
}

/// What the installation script reported for a complete write.
struct Written {
    /// The key lines it appended.
    added: usize,
    /// The file it wrote, as it reported it.
    target: String,
}

/// Sends `text`, holding `sent` key lines, to the installation script and
/// returns its report when every key was written; any other outcome stops the
/// run with what may have changed.
fn write_keys(
    ssh: &mut dyn Ssh,
    invocation: &Invocation,
    common: &[String],
    text: &[u8],
    sent: usize,
) -> Result<Written, Stop> {
    let mut install_args = vec!["-o".to_string(), "RequestTTY=no".to_string()];
    install_args.extend(common.iter().cloned());
    install_args.push(invocation.destination.clone());
    install_args.push(install_command(None));
    let installed = run_ssh(ssh, &install_args, text, false)?;
    if installed.status == Some(255) && !has_report_line(&installed.stdout) {
        return stop(
            "ssh exited with status 255 before the installation script reported \
                    anything; if authentication failed, nothing was written"
                .to_string(),
        );
    }
    let report = parse_report(&installed.stdout);
    let target = report
        .keys
        .first()
        .map(|key| String::from_utf8_lossy(&key.path).into_owned())
        .unwrap_or_else(|| TARGET.to_string());
    let added = report
        .keys
        .iter()
        .filter(|k| k.status == crate::result_line::KeyStatus::Added)
        .count();
    if report.outcome != Outcome::Unknown && report.keys.len() != sent {
        return stop(format!(
            "the remote side reported {} key(s) for {sent} sent; {target} may or may not have changed",
            report.keys.len()
        ));
    }
    match report.outcome {
        Outcome::Installed => Ok(Written { added, target }),
        Outcome::Partial => stop(format!(
            "only {added} key(s) were written to {target} before the remote side failed"
        )),
        Outcome::Unchanged => stop(format!("the key was not written to {target}")),
        Outcome::Uncertain => stop(format!(
            "writing to {target} failed and the partial line could not be removed; check the file"
        )),
        Outcome::Unknown => stop(format!(
            "the connection ended without a result; {target} may or may not have changed"
        )),
    }
}

/// Selects the keys and prints upstream's Source line: without `-i`, the keys
/// `ssh-add -L` lists, whatever `SSH_AUTH_SOCK` holds (D-20); otherwise, and
/// when the agent lists none, the selected key file after its checks, of
/// which `-f` skips only the private key's.
fn select_keys(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    err: &mut dyn Write,
) -> Result<Selection, Stop> {
    if invocation.key == KeySelection::Unspecified
        && let Some(lines) = agent_key_lines(ssh)?
    {
        info(
            err,
            &format!("Source of key(s) to be installed: {AGENT_SOURCE}"),
        );
        return Ok(Selection::Agent(lines));
    }
    let key_file = select_key_file(invocation, env, err)?;
    let public_key = key_file.public.display().to_string();
    let identity = key_file.private.display().to_string();
    let input = (env.read_file)(&key_file.public).map_err(|e| unopenable(&public_key, &e))?;
    let prepared = prepare(&input).map_err(|e| input_error(&public_key, &e))?;
    if prepared.key_count > 1 {
        return stop(format!(
            "'{public_key}' holds {} keys; a selected key file must hold one key, \
             the public half of '{identity}'",
            prepared.key_count
        ));
    }
    if !invocation.force {
        (env.readable_file)(&key_file.private)
            .map_err(|e| unopenable_private_key(&identity, &public_key, &e))?;
    }
    info(
        err,
        &format!("Source of key(s) to be installed: \"{public_key}\""),
    );
    Ok(Selection::File {
        text: prepared.text,
        identity,
        named_by_option: key_file.named_by_option,
    })
}

/// The key lines `ssh-add -L` lists, each ending in LF, when it starts, exits 0
/// and lists at least one; otherwise none. Output that fails the checks a key
/// file passes stops the run.
fn agent_key_lines(ssh: &mut dyn Ssh) -> Result<Option<Vec<Vec<u8>>>, Stop> {
    let Ok(listed) = ssh.list_agent_keys() else {
        return Ok(None);
    };
    if listed.status != Some(0) {
        return Ok(None);
    }
    match prepare(&listed.stdout) {
        Ok(prepared) => Ok(Some(
            prepared
                .text
                .split_inclusive(|&b| b == b'\n')
                .filter(|line| !matches!(line.first(), Some(b'\n' | b'#')))
                .map(<[u8]>::to_vec)
                .collect(),
        )),
        Err(InputError::NoKeys) => Ok(None),
        Err(e) => stop(input_error(AGENT_SOURCE, &e)),
    }
}

/// Runs `ssh -V` and warns when the client is not one the fixtures have tested.
fn warn_about_an_untested_client(
    ssh: &mut dyn Ssh,
    env: &Environment,
    err: &mut dyn Write,
) -> Result<(), String> {
    let version = run_ssh(ssh, &["-V".to_string()], b"", true)?;
    stop_if_interrupted(env, NOTHING_WRITTEN)?;
    let version_line = String::from_utf8_lossy(&version.stderr)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if !is_tested_client(&version_line) {
        warn(
            err,
            &format!(
                "{version_line} has not been tested with this release; output it cannot \
                 classify makes the installed-key check inconclusive"
            ),
        );
    }
    Ok(())
}

/// The identities other than `identity` that `ssh -G -i <identity>` reports the client could offer.
fn other_candidates(
    ssh: &mut dyn Ssh,
    env: &Environment,
    common: &[String],
    destination: &str,
    identity: &str,
) -> Result<Vec<String>, String> {
    let mut config_args = vec![
        "-G".to_string(),
        "-i".to_string(),
        identity.to_string(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
    ];
    config_args.extend(common.iter().cloned());
    config_args.push(destination.to_string());
    let config = run_ssh(ssh, &config_args, b"", true)?;
    stop_if_interrupted(env, NOTHING_WRITTEN)?;
    if config.status != Some(0) {
        return Err(format!(
            "ssh -G failed: {}",
            String::from_utf8_lossy(&config.stderr).trim()
        ));
    }
    Ok(other_candidates_matching(
        &String::from_utf8_lossy(&config.stdout),
        identity,
        &env.home,
        env.exists,
        env.same_file,
    ))
}

/// Prints the result of a written key's post-installation check (D-06), after `label`.
fn report_verification(err: &mut dyn Write, label: &str, verified: CheckResult, target: &str) {
    match verified {
        CheckResult::Installed => info(
            err,
            &format!("{label}the key authenticates: it is installed and verified"),
        ),
        CheckResult::NotInstalled => warn(
            err,
            &format!(
                "the key was installed but could not be verified: the server still rejects it; \
                 check the permissions of {target} and its directory"
            ),
        ),
        CheckResult::Inconclusive(reason)
        | CheckResult::Failed(reason)
        | CheckResult::NotAttempted {
            failure: reason, ..
        } => warn(
            err,
            &format!("{label}the key was installed but could not be verified: {reason}"),
        ),
    }
}

fn summary(out: &mut dyn Write, added: usize, login: &str) {
    let _ = write!(
        out,
        "\nNumber of key(s) added: {added}\n\n\
         Now try logging into the machine, with: \"{login}\"\n\
         and check to make sure that only the key(s) you wanted were added.\n\n"
    );
}

fn stop_if_interrupted(env: &Environment, message: &str) -> Result<(), String> {
    if (env.interrupted)() {
        Err(message.to_string())
    } else {
        Ok(())
    }
}

fn common_args(invocation: &Invocation, env: &Environment) -> Vec<String> {
    let mut args = vec!["-a".to_string(), "-x".to_string()];
    if batch_mode(env) {
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());
    }
    if let Some(port) = &invocation.port {
        args.push("-p".to_string());
        args.push(port.clone());
    }
    for option in &invocation.ssh_options {
        let (flag, value) = match option {
            SshOption::Option(value) => ("-o", value),
            SshOption::Config(value) => ("-F", value),
        };
        args.push(flag.to_string());
        args.push(value.clone());
    }
    args
}

fn batch_mode(env: &Environment) -> bool {
    !env.has_console && !env.askpass_set
}

/// The key file a run installs from.
struct KeyFile {
    public: PathBuf,
    private: PathBuf,
    /// Whether `-i` was given, so that the login hint names the private key,
    /// as upstream's `${SEEN_OPT_I:+-i …}` does.
    named_by_option: bool,
}

/// The key file the invocation selects, or the stop upstream reports when there
/// is none: `no ID file found` for `-i` without a file, and an empty source
/// followed by `No identities found` without `-i`, where a default key file
/// that cannot be read counts as none, as upstream's `[ -r … ]`.
fn select_key_file(
    invocation: &Invocation,
    env: &Environment,
    err: &mut dyn Write,
) -> Result<KeyFile, Stop> {
    match &invocation.key {
        KeySelection::File {
            public_key,
            private_key,
        } => Ok(KeyFile {
            public: expand_home(&env.home, public_key),
            private: expand_home(&env.home, private_key),
            named_by_option: true,
        }),
        KeySelection::DefaultFile => match default_key_file(env) {
            Some(public) => Ok(key_file_named(public, true)),
            None => stop("no ID file found"),
        },
        KeySelection::Unspecified => {
            match default_key_file(env).filter(|public| (env.readable_file)(public).is_ok()) {
                Some(public) => Ok(key_file_named(public, false)),
                None => {
                    info(err, "Source of key(s) to be installed: ");
                    stop("No identities found")
                }
            }
        }
    }
}

/// Upstream's `DEFAULT_PUB_ID_FILE` in `<home>/.ssh`; none when that directory cannot be listed.
fn default_key_file(env: &Environment) -> Option<PathBuf> {
    let dir = env.home.join(".ssh");
    let entries = (env.modification_times)(&dir).ok()?;
    newest_public_key(&entries).map(|name| dir.join(name))
}

fn key_file_named(public: PathBuf, named_by_option: bool) -> KeyFile {
    KeyFile {
        private: public.with_extension(""),
        public,
        named_by_option,
    }
}

fn expand_home(home: &Path, path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        Some(rest) => home.join(rest),
        None => path.to_path_buf(),
    }
}

fn has_report_line(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .lines()
        .any(|line| line.starts_with("ssh-copy-id:"))
}

/// The suggested login command, as upstream's: `-i` when `-i` was given and `-p`, with their values
/// unquoted, then each `-o` and `-F` and the destination single-quoted.
fn login_command(invocation: &Invocation, identity: HintIdentity) -> String {
    let mut words = vec!["ssh".to_string()];
    match identity {
        HintIdentity::Absent => {}
        HintIdentity::Bare => words.push("-i".to_string()),
        HintIdentity::Named(identity) => {
            words.push("-i".to_string());
            words.push(identity.to_string());
        }
    }
    if let Some(port) = &invocation.port {
        words.push("-p".to_string());
        words.push(port.clone());
    }
    for option in &invocation.ssh_options {
        let (flag, value) = match option {
            SshOption::Option(value) => ("-o", value),
            SshOption::Config(value) => ("-F", value),
        };
        words.push(flag.to_string());
        words.push(sh_quote(value));
    }
    words.push(sh_quote(&invocation.destination));
    words.join(" ")
}

fn probe_args(identity: &str, log: &Path, common: &[String], destination: &str) -> Vec<String> {
    let log = log.display().to_string();
    let mut args: Vec<String> = [
        "-i",
        identity,
        "-E",
        &log,
        "-o",
        "ControlPath=none",
        "-o",
        "LogLevel=VERBOSE",
        "-o",
        "PreferredAuthentications=publickey",
        "-o",
        "IdentitiesOnly=yes",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(common.iter().cloned());
    args.push(destination.to_string());
    args.push("exit".to_string());
    args
}

/// Runs one probe that logs to `log`, a path in the scratch directory, and classifies it.
fn check(
    ssh: &mut dyn Ssh,
    env: &Environment,
    args: &[String],
    log: &Path,
    others: &[String],
) -> Result<CheckResult, String> {
    let probe = run_ssh(ssh, args, b"", true)?;
    Ok(match (env.read_file)(log) {
        Ok(text) => classify(
            probe.status,
            &String::from_utf8_lossy(&text),
            &String::from_utf8_lossy(&probe.stderr),
            others,
        ),
        Err(e) => CheckResult::Inconclusive(format!(
            "cannot read the ssh log file {}: {e}",
            log.display()
        )),
    })
}

fn run_ssh(
    ssh: &mut dyn Ssh,
    args: &[String],
    stdin: &[u8],
    capture_stderr: bool,
) -> Result<SshOutput, String> {
    ssh.run(args, stdin, capture_stderr)
        .map_err(|e| format!("cannot run ssh: {e}"))
}

fn input_error(file: &str, error: &InputError) -> String {
    match error {
        InputError::PrivateKey { line } => {
            format!("'{file}' line {line} is private key material; refusing to send it")
        }
        InputError::Nul { line } => format!("'{file}' line {line} contains a NUL byte"),
        InputError::StandaloneCr { line } => {
            format!("'{file}' line {line} contains a carriage return inside the line")
        }
        InputError::Malformed { line } => format!("'{file}' line {line} is not a public key"),
        InputError::NoKeys => format!("No identities found in '{file}'"),
    }
}

/// Upstream's report of a key file that cannot be opened.
fn unopenable(file: &str, error: &io::Error) -> Stop {
    Stop::ErrorAfterBlankLine(format!(
        "failed to open ID file '{file}': {}",
        reason(error)
    ))
}

/// Upstream's report of a private key file that cannot be opened, with its
/// hint that `-f` installs the public key file without it.
fn unopenable_private_key(private_key: &str, public_key: &str, error: &io::Error) -> Stop {
    Stop::ErrorAfterBlankLine(format!(
        "failed to open ID file '{private_key}': {}\n\
         \t(to install the contents of '{public_key}' anyway, look at the -f option)",
        reason(error)
    ))
}

/// The text of `error` without the ` (os error N)` that `io::Error` appends to
/// an operating system error, so that it reads as the system's own message.
fn reason(error: &io::Error) -> String {
    let text = error.to_string();
    match error.raw_os_error() {
        Some(code) => text
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&text)
            .to_string(),
        None => text,
    }
}

fn info(err: &mut dyn Write, message: &str) {
    let _ = writeln!(err, "ssh-copy-id: INFO: {message}");
}

fn warn(err: &mut dyn Write, message: &str) {
    let _ = writeln!(err, "ssh-copy-id: WARNING: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_script::install_command;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsString;
    use std::rc::Rc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== me@here";
    const DENIED: &str = "u@h: Permission denied (publickey).\r\n";
    const ACCEPTED: &str = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".\r\n";

    struct Probe {
        output: SshOutput,
        log: Option<String>,
    }

    /// A file the run wrote through `Environment::write_file`, and its contents.
    type WrittenFile = (PathBuf, Vec<u8>);

    #[derive(Default)]
    struct FakeSsh {
        calls: Vec<(Vec<String>, Vec<u8>, bool)>,
        version: String,
        config: String,
        probes: Vec<Probe>,
        install: Option<SshOutput>,
        config_status: i32,
        logs: Rc<RefCell<HashMap<PathBuf, Vec<u8>>>>,
        scratch_parents: Rc<RefCell<Vec<PathBuf>>>,
        removed: Rc<RefCell<Vec<PathBuf>>>,
        scratch_creation_fails: bool,
        fails_to_start: Option<&'static str>,
        interrupt_during: Option<usize>,
        interrupted: Rc<Cell<bool>>,
        listing: Vec<(&'static str, u64)>,
        agent: Option<SshOutput>,
        written: Rc<RefCell<Vec<WrittenFile>>>,
    }

    impl FakeSsh {
        fn new() -> FakeSsh {
            FakeSsh {
                version: "OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2\r\n".into(),
                config: "identityfile C:/k/id\n".into(),
                ..FakeSsh::default()
            }
        }

        fn probe(self, status: i32, log: &str) -> FakeSsh {
            self.probe_with(status, Some(log), "")
        }

        fn probe_with(mut self, status: i32, log: Option<&str>, stderr: &str) -> FakeSsh {
            self.probes.push(Probe {
                output: output(status, "", stderr),
                log: log.map(str::to_string),
            });
            self
        }

        fn probe_args(&self) -> Vec<&Vec<String>> {
            self.calls
                .iter()
                .filter(|(args, _, _)| kind(args) == "probe")
                .map(|(args, _, _)| args)
                .collect()
        }

        fn write_log(&self, args: &[String], log: Option<String>) {
            let Some(at) = args.iter().position(|a| a == "-E") else {
                return;
            };
            let path = PathBuf::from(&args[at + 1]);
            let mut logs = self.logs.borrow_mut();
            match log {
                Some(text) => logs
                    .entry(path)
                    .or_default()
                    .extend_from_slice(text.as_bytes()),
                None => {
                    logs.remove(&path);
                }
            }
        }

        fn installs(self, stdout: &str) -> FakeSsh {
            self.installs_with(0, stdout)
        }

        fn installs_with(mut self, status: i32, stdout: &str) -> FakeSsh {
            self.install = Some(output(status, stdout, ""));
            self
        }

        fn kinds(&self) -> Vec<&'static str> {
            self.calls.iter().map(|(args, _, _)| kind(args)).collect()
        }

        fn call(&self, which: &str) -> &(Vec<String>, Vec<u8>, bool) {
            self.calls
                .iter()
                .find(|(args, _, _)| kind(args) == which)
                .expect("call present")
        }
    }

    fn output(status: i32, stdout: &str, stderr: &str) -> SshOutput {
        SshOutput {
            status: Some(status),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn kind(args: &[String]) -> &'static str {
        if args.first().map(String::as_str) == Some("ssh-add") {
            "agent"
        } else if args.first().map(String::as_str) == Some("-V") {
            "version"
        } else if args.first().map(String::as_str) == Some("-G") {
            "config"
        } else if args.last().map(String::as_str) == Some("exit") {
            "probe"
        } else {
            "install"
        }
    }

    impl Ssh for FakeSsh {
        fn run(
            &mut self,
            args: &[String],
            stdin: &[u8],
            capture_stderr: bool,
        ) -> io::Result<SshOutput> {
            self.calls
                .push((args.to_vec(), stdin.to_vec(), capture_stderr));
            if self.interrupt_during == Some(self.calls.len() - 1) {
                self.interrupted.set(true);
            }
            if self.fails_to_start == Some(kind(args)) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "program not found"));
            }
            Ok(match kind(args) {
                "version" => output(0, "", &self.version),
                "config" => output(self.config_status, &self.config, ""),
                "probe" => {
                    let probe = self.probes.remove(0);
                    self.write_log(args, probe.log);
                    probe.output
                }
                _ => self.install.clone().expect("install not scripted"),
            })
        }

        fn list_agent_keys(&mut self) -> io::Result<SshOutput> {
            let args = vec!["ssh-add".to_string(), "-L".to_string()];
            self.calls.push((args, Vec::new(), true));
            if self.fails_to_start == Some("agent") {
                return Err(io::Error::new(io::ErrorKind::NotFound, "program not found"));
            }
            Ok(self.agent.clone().unwrap_or_else(|| {
                output(
                    2,
                    "",
                    "Error connecting to agent: No such file or directory
",
                )
            }))
        }
    }

    const SCRATCH_NAME: &str = "ssh-copy-id.test";
    const CERTIFICATE: &str =
        "ssh-ed25519-cert-v01@openssh.com AAAAIHNzaC1lZDI1NTE5LWNlcnQ= me@here";

    fn scratch() -> PathBuf {
        Path::new("C:/home").join(".ssh").join(SCRATCH_NAME)
    }

    fn log_in_scratch(name: &str) -> String {
        scratch().join(name).display().to_string()
    }

    fn removed_only_the_scratch_directory(run: &Run) {
        assert_eq!(*run.ssh.removed.borrow(), [scratch()], "{}", run.err);
        assert!(run.ssh.logs.borrow().is_empty());
    }

    const INSTALLED: &str = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\nssh-copy-id: result=installed added=1\n";

    struct Run {
        status: i32,
        out: String,
        err: String,
        ssh: FakeSsh,
    }

    fn invocation() -> Invocation {
        Invocation {
            destination: "u@h".into(),
            key: KeySelection::File {
                public_key: PathBuf::from("C:/k/id.pub"),
                private_key: PathBuf::from("C:/k/id"),
            },
            port: None,
            ssh_options: Vec::new(),
            force: false,
        }
    }

    fn files() -> HashMap<PathBuf, Vec<u8>> {
        HashMap::from([
            (
                PathBuf::from("C:/k/id.pub"),
                format!("{KEY}\n").into_bytes(),
            ),
            (PathBuf::from("C:/k/id"), b"private".to_vec()),
        ])
    }

    fn execute_with(
        invocation: &Invocation,
        files: HashMap<PathBuf, Vec<u8>>,
        has_console: bool,
        askpass_set: bool,
        ssh: FakeSsh,
    ) -> Run {
        execute_in(
            invocation,
            files,
            HashSet::new(),
            has_console,
            askpass_set,
            ssh,
        )
    }

    fn execute_in(
        invocation: &Invocation,
        files: HashMap<PathBuf, Vec<u8>>,
        unreadable: HashSet<PathBuf>,
        has_console: bool,
        askpass_set: bool,
        mut ssh: FakeSsh,
    ) -> Run {
        let logs = Rc::clone(&ssh.logs);
        let scratch_parents = Rc::clone(&ssh.scratch_parents);
        let removed = Rc::clone(&ssh.removed);
        let written = Rc::clone(&ssh.written);
        let interrupted = Rc::clone(&ssh.interrupted);
        let scratch_creation_fails = ssh.scratch_creation_fails;
        let read_file = |p: &Path| {
            if unreadable.contains(p) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Permission denied",
                ));
            }
            files
                .get(p)
                .cloned()
                .or_else(|| logs.borrow().get(p).cloned())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No such file or directory"))
        };
        let exists = |p: &Path| files.contains_key(p);
        let readable_file = |p: &Path| {
            if unreadable.contains(p) {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Permission denied",
                ))
            } else if files.contains_key(p) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No such file or directory",
                ))
            }
        };
        let same_file = |a: &Path, b: &Path| a == b;
        let create_scratch_dir = |parent: &Path| {
            scratch_parents.borrow_mut().push(parent.to_path_buf());
            if scratch_creation_fails {
                return Err(io::Error::new(io::ErrorKind::NotFound, "not found"));
            }
            Ok(parent.join(SCRATCH_NAME))
        };
        let write_file = |p: &Path, contents: &[u8]| {
            logs.borrow_mut().insert(p.to_path_buf(), contents.to_vec());
            written
                .borrow_mut()
                .push((p.to_path_buf(), contents.to_vec()));
            Ok(())
        };
        let remove_dir = |p: &Path| {
            logs.borrow_mut().retain(|log, _| !log.starts_with(p));
            removed.borrow_mut().push(p.to_path_buf());
        };
        let is_interrupted = || interrupted.get();
        let listing: Vec<(OsString, SystemTime)> = ssh
            .listing
            .iter()
            .map(|(name, secs)| {
                (
                    OsString::from(name),
                    UNIX_EPOCH + Duration::from_secs(*secs),
                )
            })
            .collect();
        let modification_times = |dir: &Path| {
            if dir == Path::new("C:/home").join(".ssh") {
                Ok(listing.clone())
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "not found"))
            }
        };
        let env = Environment {
            has_console,
            askpass_set,
            home: PathBuf::from("C:/home"),
            read_file: &read_file,
            exists: &exists,
            readable_file: &readable_file,
            same_file: &same_file,
            create_scratch_dir: &create_scratch_dir,
            remove_dir: &remove_dir,
            interrupted: &is_interrupted,
            modification_times: &modification_times,
            write_file: &write_file,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(invocation, &env, &mut ssh, &mut out, &mut err);
        Run {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            ssh,
        }
    }

    fn execute(ssh: FakeSsh) -> Run {
        execute_with(&invocation(), files(), true, false, ssh)
    }

    #[test]
    fn a01_installed_key_is_skipped_without_writing() {
        let run = execute(FakeSsh::new().probe(0, ACCEPTED));
        assert_eq!(run.status, 0);
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe"]);
        assert!(run.err.contains("All keys were skipped"), "{}", run.err);
    }

    #[test]
    fn a02_missing_key_is_installed_and_verified() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(
            run.ssh.kinds(),
            ["version", "config", "probe", "install", "probe"]
        );
        let (args, stdin, capture_stderr) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY}\n").into_bytes());
        assert!(!capture_stderr);
        assert_eq!(args[args.len() - 2], "u@h");
        assert_eq!(args.last().unwrap(), &install_command(None));
        assert!(run.out.contains("Number of key(s) added: 1"), "{}", run.out);
        assert!(!run.err.contains("could not be verified"), "{}", run.err);
        assert!(run.err.contains("the key authenticates"), "{}", run.err);
    }

    #[test]
    fn a03_another_candidate_makes_the_check_inconclusive_and_installs() {
        let mut ssh = FakeSsh::new()
            .probe(0, ACCEPTED)
            .installs(INSTALLED)
            .probe(0, ACCEPTED);
        ssh.config = "identityfile C:/k/id\nidentityfile ~/.ssh/other\n".into();
        let mut files = files();
        files.insert(PathBuf::from("C:/home/.ssh/other"), Vec::new());
        let run = execute_with(&invocation(), files, true, false, ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(run.err.contains("~/.ssh/other"), "{}", run.err);
        assert!(run.ssh.kinds().contains(&"install"));
    }

    #[test]
    fn a04_host_key_failure_stops_before_writing() {
        let run = execute(FakeSsh::new().probe(255, "Host key verification failed.\r\n"));
        assert_eq!(run.status, 1);
        assert!(!run.ssh.kinds().contains(&"install"));
        assert!(
            run.err.contains("Host key verification failed."),
            "{}",
            run.err
        );
    }

    #[test]
    fn a05_partial_write_exits_1() {
        let partial = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\nssh-copy-id: key=2 result=failed path=.ssh/authorized_keys\nssh-copy-id: result=partial added=1\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(partial));
        assert_eq!(run.status, 1);
    }

    #[test]
    fn a50_partial_summary_for_the_one_key_exits_1_without_verifying() {
        let partial = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\nssh-copy-id: result=partial added=1\n";
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(partial)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 1);
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe", "install"]);
        assert!(!run.err.contains("installed and verified"), "{}", run.err);
        assert!(
            run.err.ends_with(
                "ssh-copy-id: ERROR: only 1 key(s) were written to \
                 .ssh/authorized_keys before the remote side failed\n"
            ),
            "{}",
            run.err
        );
    }

    #[test]
    fn a06_missing_summary_exits_1_and_says_the_state_is_unknown() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs("ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\n"),
        );
        assert_eq!(run.status, 1);
        assert!(run.err.contains("may or may not"), "{}", run.err);
    }

    #[test]
    fn a07_rejected_verification_warns_but_exits_0() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(255, DENIED),
        );
        assert_eq!(run.status, 0);
        assert!(run.err.contains("could not be verified"), "{}", run.err);
    }

    #[test]
    fn a08_private_key_input_stops_before_any_connection() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n".to_vec(),
        );
        let run = execute_with(&invocation(), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn a09_batch_mode_only_without_console_and_askpass() {
        let has_batch = |run: &Run| {
            run.ssh
                .calls
                .iter()
                .filter(|(a, _, _)| a[0] != "-V")
                .all(|(a, _, _)| {
                    a.windows(2)
                        .any(|w| w[0] == "-o" && w[1] == "BatchMode=yes")
                })
        };
        let scripted = || FakeSsh::new().probe(0, ACCEPTED);
        let no_console = execute_with(&invocation(), files(), false, false, scripted());
        assert!(has_batch(&no_console));
        assert!(
            no_console.err.contains("BatchMode=yes"),
            "{}",
            no_console.err
        );
        let with_askpass = execute_with(&invocation(), files(), false, true, scripted());
        assert!(
            !with_askpass
                .ssh
                .calls
                .iter()
                .any(|(a, _, _)| a.iter().any(|x| x == "BatchMode=yes"))
        );
        let with_console = execute_with(&invocation(), files(), true, false, scripted());
        assert!(
            !with_console
                .ssh
                .calls
                .iter()
                .any(|(a, _, _)| a.iter().any(|x| x == "BatchMode=yes"))
        );
        assert!(
            !with_console.err.contains("BatchMode=yes"),
            "{}",
            with_console.err
        );
    }

    #[test]
    fn a10_untested_client_is_warned_about() {
        let mut ssh = FakeSsh::new().probe(0, ACCEPTED);
        ssh.version = "OpenSSH_9.6p1, OpenSSL 3.0.13 30 Jan 2024\n".into();
        let run = execute(ssh);
        assert!(
            run.err.contains("WARNING: OpenSSH_9.6p1, OpenSSL"),
            "{}",
            run.err
        );
        let tested = execute(FakeSsh::new().probe(0, ACCEPTED));
        assert!(!tested.err.contains("WARNING: OpenSSH"), "{}", tested.err);
    }

    #[test]
    fn a11_probe_puts_overrides_before_user_options() {
        let mut invocation = invocation();
        invocation.port = Some("2222".into());
        invocation.ssh_options = vec![
            SshOption::Option("User=x".into()),
            SshOption::Config("cfg".into()),
        ];
        let run = execute_with(
            &invocation,
            files(),
            false,
            false,
            FakeSsh::new().probe(0, ACCEPTED),
        );
        let (args, stdin, capture_stderr) = run.ssh.call("probe");
        let log = log_in_scratch("check-1.log");
        let expected: Vec<String> = [
            "-i",
            "C:/k/id",
            "-E",
            &log,
            "-o",
            "ControlPath=none",
            "-o",
            "LogLevel=VERBOSE",
            "-o",
            "PreferredAuthentications=publickey",
            "-o",
            "IdentitiesOnly=yes",
            "-a",
            "-x",
            "-o",
            "BatchMode=yes",
            "-p",
            "2222",
            "-o",
            "User=x",
            "-F",
            "cfg",
            "u@h",
            "exit",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(args, &expected);
        assert!(stdin.is_empty());
        assert!(capture_stderr);
    }

    #[test]
    fn a12_more_than_one_key_stops_before_any_connection() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            format!("{KEY}\n{KEY}\n").into_bytes(),
        );
        let run = execute_with(&invocation(), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(run.ssh.calls.is_empty());
        assert_eq!(
            run.err,
            "ssh-copy-id: ERROR: 'C:/k/id.pub' holds 2 keys; a selected key file must hold \
             one key, the public half of 'C:/k/id'\n"
        );
    }

    #[test]
    fn a13_unreadable_key_file_exits_1() {
        let run = execute_with(&invocation(), HashMap::new(), true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(run.err.contains("C:/k/id.pub"), "{}", run.err);
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn a14_missing_private_key_stops_before_any_connection() {
        let mut files = files();
        files.remove(&PathBuf::from("C:/k/id"));
        let run = execute_with(&invocation(), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(
            run.err.contains("failed to open ID file 'C:/k/id'"),
            "{}",
            run.err
        );
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn a15_failed_write_without_any_addition_exits_1() {
        let failed = "ssh-copy-id: key=1 result=failed path=.ssh/authorized_keys\nssh-copy-id: result=unchanged added=0\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(failed));
        assert_eq!(run.status, 1);
        assert!(!run.ssh.kinds()[4..].contains(&"probe"));
    }

    #[test]
    fn a23_messages_name_the_target_the_remote_side_reported() {
        let failed = "ssh-copy-id: key=1 result=failed path=/etc/dropbear/authorized_keys\nssh-copy-id: result=unchanged added=0\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(failed));
        assert_eq!(run.status, 1);
        assert!(
            run.err.contains("/etc/dropbear/authorized_keys"),
            "{}",
            run.err
        );
    }

    #[test]
    fn a16_install_puts_request_tty_before_user_options() {
        let mut invocation = invocation();
        invocation.port = Some("2222".into());
        invocation.ssh_options = vec![
            SshOption::Option("User=x".into()),
            SshOption::Config("cfg".into()),
        ];
        let run = execute_with(
            &invocation,
            files(),
            false,
            false,
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        let (args, _, _) = run.ssh.call("install");
        let expected: Vec<String> = [
            "-o",
            "RequestTTY=no",
            "-a",
            "-x",
            "-o",
            "BatchMode=yes",
            "-p",
            "2222",
            "-o",
            "User=x",
            "-F",
            "cfg",
            "u@h",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([install_command(None)])
        .collect();
        assert_eq!(args, &expected);
    }

    #[test]
    fn a20_tilde_in_identity_is_expanded_with_home() {
        let mut invocation = invocation();
        invocation.key = KeySelection::File {
            public_key: PathBuf::from("~/k/id.pub"),
            private_key: PathBuf::from("~/k/id"),
        };
        let home = PathBuf::from("C:/home");
        let files = HashMap::from([
            (home.join("k/id.pub"), format!("{KEY}\n").into_bytes()),
            (home.join("k/id"), b"private".to_vec()),
        ]);
        let run = execute_with(
            &invocation,
            files,
            true,
            false,
            FakeSsh::new().probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        let (args, _, _) = run.ssh.call("probe");
        assert!(
            args.contains(&home.join("k/id").display().to_string()),
            "{args:?}"
        );
    }

    #[test]
    fn a21_install_exit_255_without_report_says_nothing_was_written_if_auth_failed() {
        let run = execute(FakeSsh::new().probe(255, DENIED).installs_with(255, ""));
        assert_eq!(run.status, 1);
        assert!(run.err.contains("status 255"), "{}", run.err);
        assert!(run.err.contains("nothing was written"), "{}", run.err);
    }

    #[test]
    fn a24_verification_probe_also_disables_forwarding() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        let probes: Vec<&Vec<String>> = run
            .ssh
            .calls
            .iter()
            .filter(|(args, _, _)| kind(args) == "probe")
            .map(|(args, _, _)| args)
            .collect();
        assert_eq!(probes.len(), 2);
        for args in probes {
            assert!(args.contains(&"-a".to_string()), "{args:?}");
            assert!(args.contains(&"-x".to_string()), "{args:?}");
        }
    }

    #[test]
    fn a25_comment_and_blank_lines_are_sent_but_only_keys_are_counted() {
        let mut files = files();
        let text = format!("# laptop\n\n{KEY}\n");
        files.insert(PathBuf::from("C:/k/id.pub"), text.clone().into_bytes());
        let run = execute_with(
            &invocation(),
            files,
            true,
            false,
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &text.into_bytes());
        assert!(
            run.out.contains("Number of key(s) added: 1\n"),
            "{}",
            run.out
        );
    }

    #[test]
    fn a26_unreadable_private_key_stops_before_any_connection() {
        let run = execute_in(
            &invocation(),
            files(),
            HashSet::from([PathBuf::from("C:/k/id")]),
            true,
            false,
            FakeSsh::new(),
        );
        assert_eq!(run.status, 1);
        assert!(
            run.err.contains("failed to open ID file 'C:/k/id'"),
            "{}",
            run.err
        );
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn a27_install_exit_255_after_a_report_does_not_say_nothing_was_written() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs_with(255, INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert!(!run.err.contains("nothing was written"), "{}", run.err);
    }

    #[test]
    fn a28_inconclusive_verification_is_not_reported_as_verified() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(1, ""),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(!run.err.contains("installed and verified"), "{}", run.err);
        assert!(run.err.contains("could not be verified"), "{}", run.err);
    }

    #[test]
    fn a29_unconfirmed_rollback_exits_1_and_asks_to_check_the_file() {
        let uncertain = "ssh-copy-id: key=1 result=uncertain path=.ssh/authorized_keys\nssh-copy-id: result=uncertain added=0\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(uncertain));
        assert_eq!(run.status, 1);
        assert!(run.err.contains("could not be removed"), "{}", run.err);
    }

    #[test]
    fn a30_each_probe_logs_to_its_own_file_in_a_scratch_directory_under_dot_ssh() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(
            *run.ssh.scratch_parents.borrow(),
            [Path::new("C:/home").join(".ssh")]
        );
        let logs: Vec<String> = run
            .ssh
            .probe_args()
            .iter()
            .map(|args| {
                let at = args.iter().position(|a| a == "-E").expect("-E present");
                args[at + 1].clone()
            })
            .collect();
        assert_eq!(
            logs,
            [
                log_in_scratch("check-1.log"),
                log_in_scratch("verify-1.log")
            ]
        );
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a31_a_forged_authenticated_line_on_stderr_does_not_skip_the_key() {
        let forged = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".\r\n";
        let none = "Authenticated to h ([127.0.0.1]:22) using \"none\".\r\n";
        let run = execute(
            FakeSsh::new()
                .probe_with(0, Some(none), forged)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(!run.err.contains("All keys were skipped"), "{}", run.err);
        assert!(run.ssh.kinds().contains(&"install"));
    }

    #[test]
    fn a32_an_unreadable_log_makes_the_check_inconclusive() {
        let run = execute(
            FakeSsh::new()
                .probe_with(0, None, ACCEPTED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .contains("could not tell whether the key is already installed (cannot read"),
            "{}",
            run.err
        );
        assert!(run.ssh.kinds().contains(&"install"));
    }

    #[test]
    fn a33_a_scratch_directory_that_cannot_be_created_stops_before_any_ssh_run() {
        let mut ssh = FakeSsh::new();
        ssh.scratch_creation_fails = true;
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert!(run.ssh.calls.is_empty());
        assert!(
            run.err.ends_with(
                "ssh-copy-id: ERROR: failed to create required temporary directory \
                 under ~/.ssh (HOME=\"C:/home\")\n"
            ),
            "{}",
            run.err
        );
        assert!(run.ssh.removed.borrow().is_empty());
    }

    #[test]
    fn a34_permission_denied_in_the_log_installs_the_key() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(!run.err.contains("could not tell"), "{}", run.err);
    }

    #[test]
    fn a35_verification_takes_its_evidence_from_the_log() {
        let run = execute(
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe_with(0, Some(""), ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(!run.err.contains("the key authenticates"), "{}", run.err);
        assert!(run.err.contains("could not be verified"), "{}", run.err);
    }

    #[test]
    fn a36_more_reported_keys_than_sent_exits_1_and_says_the_state_is_unknown() {
        let two = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\nssh-copy-id: key=2 result=added path=.ssh/authorized_keys\nssh-copy-id: result=installed added=2\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(two));
        assert_eq!(run.status, 1);
        assert!(
            run.err.contains(
                "the remote side reported 2 key(s) for 1 sent; \
                 .ssh/authorized_keys may or may not have changed"
            ),
            "{}",
            run.err
        );
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe", "install"]);
        assert!(!run.out.contains("Number of key(s) added"), "{}", run.out);
    }

    #[test]
    fn a37_no_reported_key_for_one_sent_exits_1_and_says_the_state_is_unknown() {
        let none = "ssh-copy-id: result=unchanged added=0\n";
        let run = execute(FakeSsh::new().probe(255, DENIED).installs(none));
        assert_eq!(run.status, 1);
        assert!(
            run.err.contains(
                "the remote side reported 0 key(s) for 1 sent; \
                 .ssh/authorized_keys may or may not have changed"
            ),
            "{}",
            run.err
        );
    }

    #[test]
    fn a38_the_configuration_query_selects_the_identity() {
        let run = execute(FakeSsh::new().probe(0, ACCEPTED));
        let (args, _, _) = run.ssh.call("config");
        assert!(
            args.windows(2).any(|w| w[0] == "-i" && w[1] == "C:/k/id"),
            "{args:?}"
        );
    }

    #[test]
    fn a22_login_suggestion_quotes_the_options() {
        let mut invocation = invocation();
        invocation.ssh_options = vec![SshOption::Option("ProxyCommand=echo 'x'".into())];
        let run = execute_with(
            &invocation,
            files(),
            true,
            false,
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.out.contains(r"-o 'ProxyCommand=echo '\''x'\'''"),
            "{}",
            run.out
        );
    }

    #[test]
    fn a39_publickey_in_the_log_skips_the_key_whatever_the_exit_status() {
        let run = execute(FakeSsh::new().probe(1, ACCEPTED));
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe"]);
        assert!(run.err.contains("All keys were skipped"), "{}", run.err);
    }

    #[test]
    fn a40_success_without_an_authenticated_line_stops_before_writing() {
        let run = execute(FakeSsh::new().probe(0, ""));
        assert_eq!(run.status, 1);
        assert!(!run.ssh.kinds().contains(&"install"));
        assert!(
            run.err
                .contains("ERROR: ssh exited without recording an authentication"),
            "{}",
            run.err
        );
    }

    #[test]
    fn a41_the_scratch_directory_is_removed_after_a_failed_probe() {
        let run = execute(FakeSsh::new().probe(255, "Host key verification failed.\r\n"));
        assert_eq!(run.status, 1);
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a42_the_scratch_directory_is_removed_when_ssh_fails_to_start() {
        let mut ssh = FakeSsh::new();
        ssh.fails_to_start = Some("version");
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert!(run.err.contains("cannot run ssh"), "{}", run.err);
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a43_the_scratch_directory_is_removed_after_an_early_error() {
        let mut ssh = FakeSsh::new();
        ssh.config_status = 255;
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert!(run.err.contains("ssh -G failed"), "{}", run.err);
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a44_the_scratch_directory_is_removed_after_the_key_is_skipped() {
        let run = execute(FakeSsh::new().probe(0, ACCEPTED));
        assert_eq!(run.status, 0, "{}", run.err);
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a45_an_interrupt_during_the_probe_stops_before_writing() {
        let mut ssh = FakeSsh::new().probe(255, DENIED).installs(INSTALLED);
        ssh.interrupt_during = Some(2);
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe"]);
        assert!(
            run.err
                .ends_with("ssh-copy-id: ERROR: interrupted; nothing was written\n"),
            "{}",
            run.err
        );
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a46_an_interrupt_during_the_configuration_query_stops_before_writing() {
        let mut ssh = FakeSsh::new().probe(255, DENIED).installs(INSTALLED);
        ssh.interrupt_during = Some(1);
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert_eq!(run.ssh.kinds(), ["version", "config"]);
        assert!(
            run.err.contains("ERROR: interrupted; nothing was written"),
            "{}",
            run.err
        );
    }

    #[test]
    fn a47_an_interrupt_during_the_installation_reports_the_add_and_skips_verification() {
        let mut ssh = FakeSsh::new()
            .probe(255, DENIED)
            .installs(INSTALLED)
            .probe(0, ACCEPTED);
        ssh.interrupt_during = Some(3);
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert_eq!(run.ssh.kinds(), ["version", "config", "probe", "install"]);
        assert!(run.out.contains("Number of key(s) added: 1"), "{}", run.out);
        assert!(
            run.err.ends_with("ssh-copy-id: ERROR: interrupted\n"),
            "{}",
            run.err
        );
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn a48_an_interrupt_during_verification_exits_1() {
        let mut ssh = FakeSsh::new()
            .probe(255, DENIED)
            .installs(INSTALLED)
            .probe(0, ACCEPTED);
        ssh.interrupt_during = Some(4);
        let run = execute(ssh);
        assert_eq!(run.status, 1);
        assert!(run.out.contains("Number of key(s) added: 1"), "{}", run.out);
        assert!(!run.err.contains("the key authenticates"), "{}", run.err);
        assert!(
            run.err
                .ends_with("ssh-copy-id: ERROR: interrupted before the key was verified\n"),
            "{}",
            run.err
        );
    }

    #[test]
    fn a49_a_certificate_is_installed_without_probing() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            format!("{CERTIFICATE}\n").into_bytes(),
        );
        let run = execute_with(
            &invocation(),
            files,
            true,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["version", "config", "install"]);
        let reason = "the key is a certificate, which authorized_keys does not authenticate";
        assert!(
            run.err.contains(&format!(
                "could not tell whether the key is already installed ({reason})"
            )),
            "{}",
            run.err
        );
        assert!(
            run.err.contains(&format!(
                "the key was installed but could not be verified: {reason}"
            )),
            "{}",
            run.err
        );
        assert!(run.out.contains("Number of key(s) added: 1"), "{}", run.out);
    }

    const ATTEMPTING: &str = "ssh-copy-id: INFO: attempting to log in with the new key(s), \
                              to filter out any that are already installed\n";

    #[test]
    fn a51_login_hint_leaves_the_identity_and_port_unquoted_as_upstream() {
        let mut invocation = invocation();
        invocation.port = Some("2222".into());
        invocation.ssh_options = vec![
            SshOption::Option("User=x".into()),
            SshOption::Config("cfg".into()),
        ];
        let run = execute_with(
            &invocation,
            files(),
            true,
            false,
            FakeSsh::new()
                .probe(255, DENIED)
                .installs(INSTALLED)
                .probe(0, ACCEPTED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(
            run.out,
            "\nNumber of key(s) added: 1\n\n\
             Now try logging into the machine, with: \
             \"ssh -i C:/k/id -p 2222 -o 'User=x' -F 'cfg' 'u@h'\"\n\
             and check to make sure that only the key(s) you wanted were added.\n\n"
        );
    }

    #[test]
    fn a52_a_missing_private_key_is_reported_as_upstream_with_the_f_hint() {
        let mut files = files();
        files.remove(&PathBuf::from("C:/k/id"));
        let run = execute_with(&invocation(), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert_eq!(
            run.err,
            "\nssh-copy-id: ERROR: failed to open ID file 'C:/k/id': No such file or directory\n\
             \t(to install the contents of 'C:/k/id.pub' anyway, look at the -f option)\n"
        );
    }

    #[test]
    fn a53_an_unreadable_private_key_names_the_reason() {
        let run = execute_in(
            &invocation(),
            files(),
            HashSet::from([PathBuf::from("C:/k/id")]),
            true,
            false,
            FakeSsh::new(),
        );
        assert_eq!(
            run.err,
            "\nssh-copy-id: ERROR: failed to open ID file 'C:/k/id': Permission denied\n\
             \t(to install the contents of 'C:/k/id.pub' anyway, look at the -f option)\n"
        );
    }

    #[test]
    fn a54_a_missing_public_key_is_reported_as_upstream() {
        let run = execute_with(&invocation(), HashMap::new(), true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert_eq!(
            run.err,
            "\nssh-copy-id: ERROR: failed to open ID file 'C:/k/id.pub': No such file or directory\n"
        );
    }

    #[test]
    fn a55_an_os_error_is_named_without_its_code() {
        let error = io::Error::from_raw_os_error(2);
        let full = error.to_string();
        assert_eq!(
            reason(&error),
            full.strip_suffix(" (os error 2)")
                .expect("Display ends with the code")
        );
        assert_eq!(
            reason(&io::Error::other("Is a directory")),
            "Is a directory"
        );
    }

    #[test]
    fn a56_skipped_keys_are_reported_between_blank_lines_with_the_f_hint() {
        let run = execute(FakeSsh::new().probe(0, ACCEPTED));
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err.ends_with(&format!(
                "{ATTEMPTING}\nssh-copy-id: WARNING: All keys were skipped because \
                 they already exist on the remote system.\n\
                 \t\t(if you think this is a mistake, you may want to use -f option)\n\n"
            )),
            "{:?}",
            run.err
        );
    }

    #[test]
    fn a57_a_probe_that_never_authenticated_relays_ssh_s_messages_as_upstream() {
        let log = "@@@@\r\nHost key verification failed.\r\n";
        let run = execute(FakeSsh::new().probe(255, log));
        assert_eq!(run.status, 1);
        assert!(!run.ssh.kinds().contains(&"install"));
        assert!(
            run.err.ends_with(&format!(
                "{ATTEMPTING}\nssh-copy-id: ERROR: @@@@\r\nERROR: Host key verification failed.\r\n\n"
            )),
            "{:?}",
            run.err
        );
    }

    #[test]
    fn a58_a_failure_with_nothing_to_relay_is_named() {
        let log = "kex_exchange_identification: Connection closed by remote host\r\n";
        let run = execute(FakeSsh::new().probe(255, log));
        assert_eq!(run.status, 1);
        assert!(
            run.err.ends_with(&format!(
                "{ATTEMPTING}ssh-copy-id: ERROR: \
                 kex_exchange_identification: Connection closed by remote host\n"
            )),
            "{:?}",
            run.err
        );
    }

    fn home_ssh(name: &str) -> PathBuf {
        Path::new("C:/home").join(".ssh").join(name)
    }

    fn selecting(key: KeySelection) -> Invocation {
        Invocation {
            key,
            ..invocation()
        }
    }

    /// `C:/home/.ssh` with `id_ed25519.pub` the newest key file other than a
    /// certificate, its private key, and an older `id_rsa.pub`.
    fn default_key_files() -> HashMap<PathBuf, Vec<u8>> {
        HashMap::from([
            (home_ssh("id_ed25519.pub"), format!("{KEY}\n").into_bytes()),
            (home_ssh("id_ed25519"), b"private".to_vec()),
            (home_ssh("id_rsa.pub"), b"ssh-rsa AAAA old@here\n".to_vec()),
            (home_ssh("id_rsa"), b"private".to_vec()),
        ])
    }

    fn with_default_key_listing(mut ssh: FakeSsh) -> FakeSsh {
        ssh.listing = vec![
            ("id_rsa.pub", 10),
            ("id_ed25519.pub", 20),
            ("id_ed25519-cert.pub", 30),
            ("config", 40),
        ];
        ssh
    }

    fn installs_and_verifies() -> FakeSsh {
        FakeSsh::new()
            .probe(255, DENIED)
            .installs(INSTALLED)
            .probe(0, ACCEPTED)
    }

    fn source_line(path: &Path) -> String {
        format!(
            "ssh-copy-id: INFO: Source of key(s) to be installed: \"{}\"\n",
            path.display()
        )
    }

    #[test]
    fn b01_without_identity_the_newest_default_key_file_is_installed() {
        let run = execute_with(
            &selecting(KeySelection::Unspecified),
            default_key_files(),
            true,
            false,
            with_default_key_listing(installs_and_verifies()),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
        let identity = home_ssh("id_ed25519").display().to_string();
        let (probe, _, _) = run.ssh.call("probe");
        assert!(
            probe.windows(2).any(|w| w[0] == "-i" && w[1] == identity),
            "{probe:?}"
        );
        let (config, _, _) = run.ssh.call("config");
        assert!(
            config.windows(2).any(|w| w[0] == "-i" && w[1] == identity),
            "{config:?}"
        );
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY}\n").into_bytes());
    }

    #[test]
    fn b02_without_identity_the_login_hint_names_no_key_as_upstream() {
        let run = execute_with(
            &selecting(KeySelection::Unspecified),
            default_key_files(),
            true,
            false,
            with_default_key_listing(installs_and_verifies()),
        );
        assert!(
            run.out
                .contains("Now try logging into the machine, with: \"ssh 'u@h'\"\n"),
            "{}",
            run.out
        );
    }

    #[test]
    fn b03_identity_without_a_file_installs_the_default_key_file() {
        let run = execute_with(
            &selecting(KeySelection::DefaultFile),
            default_key_files(),
            true,
            false,
            with_default_key_listing(installs_and_verifies()),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
        let hint = format!(
            "Now try logging into the machine, with: \"ssh -i {} 'u@h'\"\n",
            home_ssh("id_ed25519").display()
        );
        assert!(run.out.contains(&hint), "{}", run.out);
    }

    #[test]
    fn b04_identity_without_a_file_and_no_default_key_file_stops_as_upstream() {
        let mut ssh = FakeSsh::new();
        ssh.listing = vec![("id_ed25519-cert.pub", 30), ("config", 40)];
        let run = execute_with(
            &selecting(KeySelection::DefaultFile),
            default_key_files(),
            true,
            false,
            ssh,
        );
        assert_eq!(run.status, 1);
        assert_eq!(run.err, "ssh-copy-id: ERROR: no ID file found\n");
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn b05_no_identity_and_no_default_key_file_finds_no_identities_as_upstream() {
        let run = execute_with(
            &selecting(KeySelection::Unspecified),
            HashMap::new(),
            true,
            false,
            FakeSsh::new(),
        );
        assert_eq!(run.status, 1);
        assert_eq!(
            run.err,
            "ssh-copy-id: INFO: Source of key(s) to be installed: \n\
             ssh-copy-id: ERROR: No identities found\n"
        );
        assert_eq!(run.ssh.kinds(), ["agent"]);
        assert!(run.ssh.scratch_parents.borrow().is_empty());
    }

    #[test]
    fn b06_without_identity_an_unreadable_default_key_file_counts_as_none() {
        let run = execute_in(
            &selecting(KeySelection::Unspecified),
            default_key_files(),
            HashSet::from([home_ssh("id_ed25519.pub")]),
            true,
            false,
            with_default_key_listing(FakeSsh::new()),
        );
        assert_eq!(run.status, 1);
        assert!(
            run.err
                .ends_with("ssh-copy-id: ERROR: No identities found\n"),
            "{}",
            run.err
        );
    }

    #[test]
    fn b07_identity_without_a_file_reports_an_unreadable_default_key_file() {
        let run = execute_in(
            &selecting(KeySelection::DefaultFile),
            default_key_files(),
            HashSet::from([home_ssh("id_ed25519.pub")]),
            true,
            false,
            with_default_key_listing(FakeSsh::new()),
        );
        assert_eq!(run.status, 1);
        assert_eq!(
            run.err,
            format!(
                "\nssh-copy-id: ERROR: failed to open ID file '{}': Permission denied\n",
                home_ssh("id_ed25519.pub").display()
            )
        );
    }

    #[test]
    fn b08_the_default_key_file_needs_its_private_key() {
        let mut files = default_key_files();
        files.remove(&home_ssh("id_ed25519"));
        let run = execute_with(
            &selecting(KeySelection::Unspecified),
            files,
            true,
            false,
            with_default_key_listing(FakeSsh::new()),
        );
        assert_eq!(run.status, 1);
        assert_eq!(
            run.err,
            format!(
                "\nssh-copy-id: ERROR: failed to open ID file '{}': No such file or directory\n\
                 \t(to install the contents of '{}' anyway, look at the -f option)\n",
                home_ssh("id_ed25519").display(),
                home_ssh("id_ed25519.pub").display()
            )
        );
        assert_eq!(run.ssh.kinds(), ["agent"]);
    }

    const KEY2: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ== other@here";
    const AGENT_SOURCE: &str = "ssh-copy-id: INFO: Source of key(s) to be installed: ssh-add -L\n";
    const INSTALLED_TWO: &str = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\n\
                                 ssh-copy-id: key=2 result=added path=.ssh/authorized_keys\n\
                                 ssh-copy-id: result=installed added=2\n";

    fn listing_agent_keys(mut ssh: FakeSsh, stdout: &str) -> FakeSsh {
        ssh.agent = Some(output(0, stdout, ""));
        ssh
    }

    fn two_agent_keys(ssh: FakeSsh) -> FakeSsh {
        listing_agent_keys(ssh, &format!("{KEY}\n{KEY2}\n"))
    }

    fn agent_key_file(number: usize) -> String {
        log_in_scratch(&format!("agent-key-{number}.pub"))
    }

    fn identity_of(args: &[String]) -> &str {
        let at = args.iter().position(|a| a == "-i").expect("-i present");
        &args[at + 1]
    }

    fn without_identity(ssh: FakeSsh) -> Run {
        execute_with(
            &selecting(KeySelection::Unspecified),
            default_key_files(),
            true,
            false,
            with_default_key_listing(ssh),
        )
    }

    #[test]
    fn c01_without_identity_the_agent_keys_are_installed_together() {
        let ssh = two_agent_keys(
            FakeSsh::new()
                .probe(255, DENIED)
                .probe(255, DENIED)
                .installs(INSTALLED_TWO)
                .probe(0, ACCEPTED)
                .probe(0, ACCEPTED),
        );
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(
            run.ssh.kinds(),
            [
                "agent", "version", "config", "config", "probe", "probe", "install", "probe",
                "probe"
            ]
        );
        assert!(run.err.starts_with(AGENT_SOURCE), "{}", run.err);
        assert!(
            run.err
                .contains("ssh-copy-id: INFO: 2 key(s) remain to be installed"),
            "{}",
            run.err
        );
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY}\n{KEY2}\n").into_bytes());
        assert!(
            run.out.contains("Number of key(s) added: 2\n"),
            "{}",
            run.out
        );
        assert!(
            run.out
                .contains("Now try logging into the machine, with: \"ssh 'u@h'\"\n"),
            "{}",
            run.out
        );
    }

    #[test]
    fn c02_each_agent_key_is_checked_alone_from_its_own_file_in_the_scratch_directory() {
        let ssh = two_agent_keys(
            FakeSsh::new()
                .probe(255, DENIED)
                .probe(255, DENIED)
                .installs(INSTALLED_TWO)
                .probe(0, ACCEPTED)
                .probe(0, ACCEPTED),
        );
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        let identities = |which: &str| -> Vec<String> {
            run.ssh
                .calls
                .iter()
                .filter(|(args, _, _)| kind(args) == which)
                .map(|(args, _, _)| identity_of(args).to_string())
                .collect()
        };
        let expected = [agent_key_file(1), agent_key_file(2)];
        assert_eq!(identities("config"), expected);
        assert_eq!(identities("probe"), [&expected[..], &expected[..]].concat());
    }

    #[test]
    fn c03_the_agent_key_files_hold_one_line_and_go_with_the_scratch_directory() {
        let ssh = two_agent_keys(FakeSsh::new().probe(0, ACCEPTED).probe(0, ACCEPTED));
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(
            *run.ssh.written.borrow(),
            [
                (
                    PathBuf::from(agent_key_file(1)),
                    format!("{KEY}\n").into_bytes()
                ),
                (
                    PathBuf::from(agent_key_file(2)),
                    format!("{KEY2}\n").into_bytes()
                ),
            ]
        );
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn c04_installed_agent_keys_are_skipped_and_the_others_installed() {
        let one = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\n\
                   ssh-copy-id: result=installed added=1\n";
        let ssh = two_agent_keys(
            FakeSsh::new()
                .probe(0, ACCEPTED)
                .probe(255, DENIED)
                .installs(one)
                .probe(0, ACCEPTED),
        );
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY2}\n").into_bytes());
        assert!(
            run.err
                .contains("ssh-copy-id: INFO: 1 key(s) remain to be installed"),
            "{}",
            run.err
        );
        let probes = run.ssh.probe_args();
        assert_eq!(identity_of(probes[2]), agent_key_file(2));
        assert!(
            run.err.contains(
                "ssh-copy-id: INFO: key 2 from ssh-add -L: the key authenticates: \
                 it is installed and verified\n"
            ),
            "{}",
            run.err
        );
        assert!(
            run.out.contains("Number of key(s) added: 1\n"),
            "{}",
            run.out
        );
    }

    #[test]
    fn c05_all_agent_keys_installed_skips_them_all() {
        let run = without_identity(two_agent_keys(
            FakeSsh::new().probe(0, ACCEPTED).probe(0, ACCEPTED),
        ));
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(!run.ssh.kinds().contains(&"install"));
        assert!(run.err.contains("All keys were skipped"), "{}", run.err);
    }

    #[test]
    fn c06_an_agent_without_keys_leaves_the_default_key_file() {
        let mut ssh = installs_and_verifies();
        ssh.agent = Some(output(1, "The agent has no identities.\n", ""));
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
    }

    #[test]
    fn c07_an_unreachable_agent_leaves_the_default_key_file() {
        let run = without_identity(installs_and_verifies());
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds()[0], "agent");
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
    }

    #[test]
    fn c08_ssh_add_that_cannot_start_leaves_the_default_key_file() {
        let mut ssh = installs_and_verifies();
        ssh.fails_to_start = Some("agent");
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
    }

    #[test]
    fn c09_an_empty_agent_listing_leaves_the_default_key_file() {
        let run = without_identity(listing_agent_keys(installs_and_verifies(), ""));
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err
                .starts_with(&source_line(&home_ssh("id_ed25519.pub"))),
            "{}",
            run.err
        );
    }

    #[test]
    fn c10_agent_output_is_validated_like_a_key_file() {
        let run = without_identity(listing_agent_keys(FakeSsh::new(), "not a key\n"));
        assert_eq!(run.status, 1);
        assert_eq!(run.ssh.kinds(), ["agent"]);
        assert_eq!(
            run.err,
            "ssh-copy-id: ERROR: 'ssh-add -L' line 1 is not a public key\n"
        );
    }

    #[test]
    fn c11_identity_given_or_not_the_agent_is_not_consulted_with_i() {
        for key in [invocation().key, KeySelection::DefaultFile] {
            let run = execute_with(
                &selecting(key),
                default_key_files().into_iter().chain(files()).collect(),
                true,
                false,
                with_default_key_listing(two_agent_keys(installs_and_verifies())),
            );
            assert_eq!(run.status, 0, "{}", run.err);
            assert!(!run.ssh.kinds().contains(&"agent"), "{:?}", run.ssh.kinds());
        }
    }

    #[test]
    fn c12_several_agent_keys_name_the_key_in_a_check_warning() {
        let ssh = two_agent_keys(
            FakeSsh::new()
                .probe(255, DENIED)
                .probe(1, "")
                .installs(INSTALLED_TWO)
                .probe(0, ACCEPTED)
                .probe(0, ACCEPTED),
        );
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err.contains(
                "ssh-copy-id: WARNING: key 2 from ssh-add -L: could not tell whether the key \
                 is already installed"
            ),
            "{}",
            run.err
        );
    }

    #[test]
    fn c13_one_agent_key_is_reported_without_its_number() {
        let ssh = listing_agent_keys(installs_and_verifies(), &format!("{KEY}\n"));
        let run = without_identity(ssh);
        assert_eq!(run.status, 0, "{}", run.err);
        assert!(
            run.err.contains(
                "ssh-copy-id: INFO: the key authenticates: it is installed and verified\n"
            ),
            "{}",
            run.err
        );
    }

    #[test]
    fn c14_each_check_logs_to_a_file_named_after_its_key() {
        let one = "ssh-copy-id: key=1 result=added path=.ssh/authorized_keys\n\
                   ssh-copy-id: result=installed added=1\n";
        let ssh = two_agent_keys(
            FakeSsh::new()
                .probe(0, ACCEPTED)
                .probe(255, DENIED)
                .installs(one)
                .probe(0, ACCEPTED),
        );
        let run = without_identity(ssh);
        let logs: Vec<String> = run
            .ssh
            .probe_args()
            .iter()
            .map(|args| {
                let at = args.iter().position(|a| a == "-E").expect("-E present");
                args[at + 1].clone()
            })
            .collect();
        assert_eq!(
            logs,
            [
                log_in_scratch("check-1.log"),
                log_in_scratch("check-2.log"),
                log_in_scratch("verify-2.log")
            ]
        );
    }

    fn forced(invocation: Invocation) -> Invocation {
        Invocation {
            force: true,
            ..invocation
        }
    }

    fn summary_naming(login: &str, added: usize) -> String {
        format!(
            "\nNumber of key(s) added: {added}\n\n\
             Now try logging into the machine, with: \"{login}\"\n\
             and check to make sure that only the key(s) you wanted were added.\n\n"
        )
    }

    #[test]
    fn d01_force_installs_without_any_check_or_verification() {
        let run = execute_with(
            &forced(invocation()),
            files(),
            true,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["install"]);
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY}\n").into_bytes());
        assert_eq!(run.err, source_line(Path::new("C:/k/id.pub")));
        removed_only_the_scratch_directory(&run);
    }

    #[test]
    fn d02_force_does_not_need_the_private_key() {
        let mut files = files();
        files.remove(&PathBuf::from("C:/k/id"));
        let run = execute_with(
            &forced(invocation()),
            files,
            true,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["install"]);
    }

    #[test]
    fn d03_force_login_hint_names_identity_without_a_file_as_upstream() {
        let mut invocation = forced(invocation());
        invocation.port = Some("2222".into());
        let run = execute_with(
            &invocation,
            files(),
            true,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.out, summary_naming("ssh -i -p 2222 'u@h'", 1));
    }

    #[test]
    fn d04_force_still_rejects_private_key_input() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n".to_vec(),
        );
        let run = execute_with(&forced(invocation()), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(run.ssh.calls.is_empty());
        assert!(run.err.contains("private key material"), "{}", run.err);
    }

    #[test]
    fn d05_force_still_rejects_a_key_file_with_two_keys() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            format!("{KEY}\n{KEY2}\n").into_bytes(),
        );
        let run = execute_with(&forced(invocation()), files, true, false, FakeSsh::new());
        assert_eq!(run.status, 1);
        assert!(run.ssh.calls.is_empty());
    }

    #[test]
    fn d06_force_installs_every_agent_key_without_writing_key_files() {
        let run = execute_with(
            &forced(selecting(KeySelection::Unspecified)),
            default_key_files(),
            true,
            false,
            with_default_key_listing(two_agent_keys(FakeSsh::new().installs(INSTALLED_TWO))),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["agent", "install"]);
        let (_, stdin, _) = run.ssh.call("install");
        assert_eq!(stdin, &format!("{KEY}\n{KEY2}\n").into_bytes());
        assert_eq!(run.err, AGENT_SOURCE);
        assert_eq!(run.out, summary_naming("ssh 'u@h'", 2));
        assert!(run.ssh.written.borrow().is_empty());
    }

    #[test]
    fn d07_force_installs_the_default_key_file_without_its_private_key() {
        let mut files = default_key_files();
        files.remove(&home_ssh("id_ed25519"));
        let run = execute_with(
            &forced(selecting(KeySelection::Unspecified)),
            files,
            true,
            false,
            with_default_key_listing(FakeSsh::new().installs(INSTALLED)),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["agent", "install"]);
        assert_eq!(run.out, summary_naming("ssh 'u@h'", 1));
    }

    #[test]
    fn d08_force_installs_a_certificate_without_a_warning() {
        let mut files = files();
        files.insert(
            PathBuf::from("C:/k/id.pub"),
            format!("{CERTIFICATE}\n").into_bytes(),
        );
        let run = execute_with(
            &forced(invocation()),
            files,
            true,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        assert_eq!(run.ssh.kinds(), ["install"]);
        assert!(!run.err.contains("WARNING"), "{}", run.err);
    }

    #[test]
    fn d09_force_reports_a_failed_write_as_without_force() {
        let failed = "ssh-copy-id: key=1 result=failed path=.ssh/authorized_keys\nssh-copy-id: result=unchanged added=0\n";
        let run = execute_with(
            &forced(invocation()),
            files(),
            true,
            false,
            FakeSsh::new().installs(failed),
        );
        assert_eq!(run.status, 1);
        assert!(
            run.err
                .ends_with("ssh-copy-id: ERROR: the key was not written to .ssh/authorized_keys\n"),
            "{}",
            run.err
        );
    }

    #[test]
    fn d10_force_reports_an_interrupt_during_the_installation_after_the_summary() {
        let mut ssh = FakeSsh::new().installs(INSTALLED);
        ssh.interrupt_during = Some(0);
        let run = execute_with(&forced(invocation()), files(), true, false, ssh);
        assert_eq!(run.status, 1);
        assert!(run.out.contains("Number of key(s) added: 1"), "{}", run.out);
        assert!(
            run.err.ends_with("ssh-copy-id: ERROR: interrupted\n"),
            "{}",
            run.err
        );
    }

    #[test]
    fn d11_force_still_runs_in_batch_mode_without_a_console() {
        let run = execute_with(
            &forced(invocation()),
            files(),
            false,
            false,
            FakeSsh::new().installs(INSTALLED),
        );
        assert_eq!(run.status, 0, "{}", run.err);
        let (args, _, _) = run.ssh.call("install");
        assert!(
            args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]),
            "{args:?}"
        );
        assert!(run.err.contains("BatchMode=yes"), "{}", run.err);
    }
}
