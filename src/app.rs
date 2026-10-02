//! One run of the CLI: the stage 1 flow from the selected key to the reported outcome.

use crate::cli_args::{Invocation, SshOption};
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
    /// Whether a path names a regular file this process can open for reading.
    pub readable_file: &'a dyn Fn(&Path) -> bool,
    /// Whether two paths name the same file.
    pub same_file: &'a dyn Fn(&Path, &Path) -> bool,
    /// Creates a new directory, readable only by its owner, inside the given
    /// existing directory and returns its path; the probes' `ssh -E` logs go there.
    pub create_scratch_dir: &'a dyn Fn(&Path) -> io::Result<PathBuf>,
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

/// Runs the stage 1 flow and returns the exit status: 0 when the key is installed
/// or was already installed, 1 otherwise. Diagnostics go to `err`, and the final
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
        Err(message) => {
            let _ = writeln!(err, "ssh-copy-id: ERROR: {message}");
            1
        }
    }
}

fn install(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), String> {
    let public_path = expand_home(&env.home, &invocation.public_key);
    let private_path = expand_home(&env.home, &invocation.private_key);
    let public_key = public_path.display().to_string();
    let identity = private_path.display().to_string();
    let input = (env.read_file)(&public_path)
        .map_err(|e| format!("failed to open ID file '{public_key}': {e}"))?;
    let prepared = prepare(&input).map_err(|e| input_error(&public_key, &e))?;
    if prepared.key_count > 1 {
        return Err(format!(
            "'{public_key}' contains {} keys; this release installs one key per run",
            prepared.key_count
        ));
    }
    if !(env.readable_file)(&private_path) {
        return Err(format!("failed to open ID file '{identity}'"));
    }
    info(
        err,
        &format!("Source of key(s) to be installed: \"{public_key}\""),
    );
    let scratch = ScratchDir {
        path: (env.create_scratch_dir)(&env.home.join(".ssh")).map_err(|_| {
            format!(
                "failed to create required temporary directory under ~/.ssh (HOME=\"{}\")",
                env.home.display()
            )
        })?,
        remove: env.remove_dir,
    };

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

    let common = common_args(invocation, env);
    if batch_mode(env) {
        info(
            err,
            "there is no console and SSH_ASKPASS is not set, so ssh runs with BatchMode=yes; \
             password and passphrase prompts fail",
        );
    }
    let mut config_args = vec![
        "-G".to_string(),
        "-i".to_string(),
        identity.clone(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
    ];
    config_args.extend(common.iter().cloned());
    config_args.push(invocation.destination.clone());
    let config = run_ssh(ssh, &config_args, b"", true)?;
    stop_if_interrupted(env, NOTHING_WRITTEN)?;
    if config.status != Some(0) {
        return Err(format!(
            "ssh -G failed: {}",
            String::from_utf8_lossy(&config.stderr).trim()
        ));
    }
    let others = other_candidates_matching(
        &String::from_utf8_lossy(&config.stdout),
        &identity,
        &env.home,
        env.exists,
        env.same_file,
    );
    let certificate = is_certificate(&prepared.text);
    let probe = |ssh: &mut dyn Ssh, log_name: &str| {
        if certificate {
            return Ok(CheckResult::Inconclusive(CERTIFICATE_REASON.to_string()));
        }
        let log = scratch.path.join(log_name);
        let args = probe_args(&identity, &log, &common, &invocation.destination);
        check(ssh, env, &args, &log, &others)
    };

    info(
        err,
        "attempting to log in with the new key(s), to filter out any that are already installed",
    );
    let checked = probe(ssh, "check.log")?;
    stop_if_interrupted(env, NOTHING_WRITTEN)?;
    match checked {
        CheckResult::Installed => {
            warn(
                err,
                "All keys were skipped because they already exist on the remote system.",
            );
            return Ok(());
        }
        CheckResult::Failed(message) => return Err(message),
        CheckResult::Inconclusive(reason) => warn(
            err,
            &format!(
                "could not tell whether the key is already installed ({reason}); \
                 installing it, which may add a duplicate"
            ),
        ),
        CheckResult::NotInstalled => {}
    }

    info(
        err,
        "1 key(s) remain to be installed -- if you are prompted now it is to install the new keys",
    );
    let mut install_args = vec!["-o".to_string(), "RequestTTY=no".to_string()];
    install_args.extend(common.iter().cloned());
    install_args.push(invocation.destination.clone());
    install_args.push(install_command(None));
    let installed = run_ssh(ssh, &install_args, &prepared.text, false)?;
    if installed.status == Some(255) && !has_report_line(&installed.stdout) {
        return Err(
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
    if report.outcome != Outcome::Unknown && report.keys.len() != prepared.key_count {
        return Err(format!(
            "the remote side reported {} key(s) for {} sent; {target} may or may not have changed",
            report.keys.len(),
            prepared.key_count
        ));
    }
    match report.outcome {
        Outcome::Installed => {}
        Outcome::Partial => {
            return Err(format!(
                "only {added} key(s) were written to {target} before the remote side failed"
            ));
        }
        Outcome::Unchanged => return Err(format!("the key was not written to {target}")),
        Outcome::Uncertain => {
            return Err(format!(
                "writing to {target} failed and the partial line could not be removed; check the file"
            ));
        }
        Outcome::Unknown => {
            return Err(format!(
                "the connection ended without a result; {target} may or may not have changed"
            ));
        }
    }

    let login = login_command(invocation, &identity);
    if (env.interrupted)() {
        summary(out, added, &login);
        return Err("interrupted".to_string());
    }
    let verified = probe(ssh, "verify.log")?;
    if (env.interrupted)() {
        summary(out, added, &login);
        return Err("interrupted before the key was verified".to_string());
    }
    match verified {
        CheckResult::Installed => info(err, "the key authenticates: it is installed and verified"),
        CheckResult::NotInstalled => warn(
            err,
            &format!(
                "the key was installed but could not be verified: the server still rejects it; \
                 check the permissions of {target} and its directory"
            ),
        ),
        CheckResult::Inconclusive(reason) | CheckResult::Failed(reason) => warn(
            err,
            &format!("the key was installed but could not be verified: {reason}"),
        ),
    }

    summary(out, added, &login);
    Ok(())
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

fn login_command(invocation: &Invocation, identity: &str) -> String {
    let mut words = vec!["ssh".to_string(), "-i".to_string(), sh_quote(identity)];
    if let Some(port) = &invocation.port {
        words.push("-p".to_string());
        words.push(sh_quote(port));
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
    use std::rc::Rc;

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== me@here";
    const DENIED: &str = "u@h: Permission denied (publickey).\r\n";
    const ACCEPTED: &str = "Authenticated to h ([127.0.0.1]:22) using \"publickey\".\r\n";

    struct Probe {
        output: SshOutput,
        log: Option<String>,
    }

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
        if args.first().map(String::as_str) == Some("-V") {
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
            public_key: PathBuf::from("C:/k/id.pub"),
            private_key: PathBuf::from("C:/k/id"),
            port: None,
            ssh_options: Vec::new(),
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
        let interrupted = Rc::clone(&ssh.interrupted);
        let scratch_creation_fails = ssh.scratch_creation_fails;
        let read_file = |p: &Path| {
            files
                .get(p)
                .cloned()
                .or_else(|| logs.borrow().get(p).cloned())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
        };
        let exists = |p: &Path| files.contains_key(p);
        let readable_file = |p: &Path| files.contains_key(p) && !unreadable.contains(p);
        let same_file = |a: &Path, b: &Path| a == b;
        let create_scratch_dir = |parent: &Path| {
            scratch_parents.borrow_mut().push(parent.to_path_buf());
            if scratch_creation_fails {
                return Err(io::Error::new(io::ErrorKind::NotFound, "not found"));
            }
            Ok(parent.join(SCRATCH_NAME))
        };
        let remove_dir = |p: &Path| {
            logs.borrow_mut().retain(|log, _| !log.starts_with(p));
            removed.borrow_mut().push(p.to_path_buf());
        };
        let is_interrupted = || interrupted.get();
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
        assert!(!run.err.contains("-f"), "{}", run.err);
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
        let log = log_in_scratch("check.log");
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
        invocation.public_key = PathBuf::from("~/k/id.pub");
        invocation.private_key = PathBuf::from("~/k/id");
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
            [log_in_scratch("check.log"), log_in_scratch("verify.log")]
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
    fn a22_login_suggestion_quotes_every_value() {
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
}
