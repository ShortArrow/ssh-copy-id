//! The keys a run installs: from the agent, from `-i`, or from the default key file.

use super::{
    Environment, Ssh, Stop, info, input_error, reason, stop, unopenable, unopenable_private_key,
};
use crate::cli_args::{Force, Invocation, KeySelection};
use crate::default_key::newest_public_key;
use crate::key_input::{InputError, PreparedInput, key_lines, prepare, prepare_verbatim};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The keys a run installs, selected before any connection.
pub(super) enum Selection {
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
pub(super) struct SelectedKey {
    /// Its position among the key lines of its source, from 1.
    pub(super) number: usize,
    /// The lines sent when it is installed, each ending in LF.
    pub(super) text: Vec<u8>,
    /// The `-i` argument of its installed-key check: the private key file, or
    /// for an agent key the one-line public key file written for it.
    pub(super) identity: String,
    /// What precedes a message about it: empty unless its source holds several keys.
    pub(super) label: String,
}

const AGENT_SOURCE: &str = "ssh-add -L";

/// What the login hint says about `-i`, as upstream's
/// `${SEEN_OPT_I:+-i${PRIV_ID_FILE:+ $PRIV_ID_FILE} }`.
pub(super) enum HintIdentity<'a> {
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
    /// it, and `-i` alone when `-f` preceded that selection.
    pub(super) fn hint_identity(&self, force: Force) -> HintIdentity<'_> {
        match self {
            Selection::File {
                named_by_option: true,
                ..
            } if force == Force::BeforeKeySelection => HintIdentity::Bare,
            Selection::File {
                identity,
                named_by_option: true,
                ..
            } => HintIdentity::Named(identity),
            _ => HintIdentity::Absent,
        }
    }

    /// The lines of every selected key, in order, as they are sent.
    pub(super) fn texts(&self) -> Vec<Vec<u8>> {
        match self {
            Selection::File { text, .. } => vec![text.clone()],
            Selection::Agent(lines) => lines.clone(),
        }
    }

    /// The keys to check, each agent key written as `agent-key-<n>.pub` in `scratch`, as upstream's `L_TMP_ID_FILE`.
    pub(super) fn keys(
        &self,
        scratch: &Path,
        env: &Environment,
    ) -> Result<Vec<SelectedKey>, String> {
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

/// Selects the keys and prints upstream's Source line: without `-i`, the keys
/// `ssh-add -L` lists, whatever `SSH_AUTH_SOCK` holds (D-20); otherwise, and
/// when the agent lists none, the selected key file after its checks, of
/// which `-f` before the key selection skips only the private key's. Under `-f`
/// the lines are taken as given, as upstream's `$(cat file)` and `$(ssh-add -L)`
/// take them; otherwise they are trimmed as upstream's `read -r` does.
pub(super) fn select_keys(
    invocation: &Invocation,
    env: &Environment,
    ssh: &mut dyn Ssh,
    err: &mut dyn Write,
) -> Result<Selection, Stop> {
    let read_lines = line_reader(invocation.force);
    if invocation.key == KeySelection::Unspecified
        && let Some(lines) = agent_key_lines(ssh, read_lines)?
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
    let prepared = read_lines(&input).map_err(|e| input_error(&public_key, &e))?;
    if prepared.key_count > 1 {
        return stop(format!(
            "'{public_key}' holds {} keys; a selected key file must hold one key, \
             the public half of '{identity}'",
            prepared.key_count
        ));
    }
    if invocation.force != Force::BeforeKeySelection {
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

/// Validates and normalizes key input for one run.
type LineReader = fn(&[u8]) -> Result<PreparedInput, InputError>;

/// The reader for `force`: lines as given under `-f`, trimmed otherwise.
fn line_reader(force: Force) -> LineReader {
    if force.is_on() {
        prepare_verbatim
    } else {
        prepare
    }
}

/// The key lines `ssh-add -L` lists, each ending in LF, when it starts, exits 0
/// and lists at least one; otherwise none. Output that fails the checks a key
/// file passes stops the run.
fn agent_key_lines(
    ssh: &mut dyn Ssh,
    read_lines: LineReader,
) -> Result<Option<Vec<Vec<u8>>>, Stop> {
    let Ok(listed) = ssh.list_agent_keys() else {
        return Ok(None);
    };
    if listed.status != Some(0) {
        return Ok(None);
    }
    match read_lines(&listed.stdout) {
        Ok(prepared) => Ok(Some(
            key_lines(&prepared.text)
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect(),
        )),
        Err(InputError::NoKeys) => Ok(None),
        Err(e) => stop(input_error(AGENT_SOURCE, &e)),
    }
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
