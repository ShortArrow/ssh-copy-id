# Adversarial Design Review

Date: 2026-09-20. Scope: [design.md](design.md), before implementation.
A second round on 2026-09-24 is dispositioned [at the end](#second-round-2026-09-24).

Accepted differences are indexed in [Differences from Upstream](compatibility.md).

The original review was a document and upstream-behavior review, not a security
certification. A subsequent Docker experiment is linked under P1-1;
`src/main.rs` is still a placeholder.
Recommendations below are proposed decisions unless a subsequent decision is
explicitly recorded, as in P1-1, P1-2, P1-4, P1-5, P2-1, and P2-2.

## Assessment

The functional scope is clear enough to start prototypes, but authentication
classification and file mutation contracts need decisions before production
implementation. Minimizing dependencies does not by itself settle these issues.

Priorities: P1 means potential incorrect authorization, destructive updates, or
false success; P2 means a compatibility or operational contract requiring a decision.

## P1-1: A Successful Probe May Use a Different Identity

Affected section: Compatibility with the Linux Version.

Counterexample: key A is selected for installation; the host configuration also
contains `IdentityFile B`, and B already authenticates. `-i A` with
`IdentitiesOnly=yes` does not necessarily isolate A: configured identity files
are additive. Certificates and connection reuse introduce additional paths to
success. A can be skipped without establishing that A authenticated.

Subsequent decision (2026-09-20): record the project's selected-key check as a
behavioral difference from upstream, not as an upstream defect or bug fix.
Upstream author intent is unconfirmed. Authentication with another key will not
establish that the selected key is installed; implementation and inconclusive-result
handling remain open. See the corresponding decision in [design.md](design.md#recorded-difference-identity-used-for-the-installed-key-check).

Decision needed: define how the probe proves use of the selected identity while
preserving host, proxy, and trust settings. Do not assume `-F none` is an adequate
fix; it also discards those settings. Distinguish target authentication from
authentication to a jump host. Evaluate whether the chosen process backend can
provide sufficient evidence without depending on unstable debug text.

Acceptance test: A is absent, B works, and B is configured or loaded in the agent;
A must not be reported as installed. Repeat with a certificate and an existing
multiplexed connection. Jump-host credentials must still work.

Basis: [IdentityFile and IdentitiesOnly](https://man.openbsd.org/ssh_config#IdentityFile).

Validation update: the pinned script skipped the selected, absent key in both
direct and single-jump Docker fixtures. A generated configuration isolated the
selected key while retaining the fixture's jump credentials and target host-key
verification. See [results and limits](design.md#selected-identity-experiment-results).

## P1-2: Authentication, Session Execution, and Key Presence Are Different States

Affected sections: Compatibility; Execution Flow; SFTP Mode.

Counterexamples: an installed key has a forced command that exits nonzero; an
SFTP subsystem is unavailable after authentication; MFA requires another factor;
or an agent refuses a signature. None establishes that the key is absent.
Appending an unrestricted copy of a restricted key can broaden access where the
new entry is accepted. Conversely, successful authentication does not prove the
key exists in the particular file selected by `-t`.

Subsequent decision (2026-09-20): keep the remote `exit` check in normal mode;
use successful SFTP session establishment in `-s` mode, without remote commands
or the upstream SFTP-only error-message shortcut. Record this as a design
difference, not a bug fix. The selected-identity requirement applies to both.
See [design.md](design.md#recorded-difference-sftp-installed-key-check).

The broader authentication-state proposal below has not been adopted as a
replacement for normal-mode checking. Failure classification remains open.

Original review proposal: represent authenticated, explicit rejection, session failure,
and indeterminate results separately. An SSH process exit code alone is not an
authentication API. Do not automatically retry with `-f` or append after an
ambiguous failure. Specify the compatibility limitation for forced commands,
MFA, and `-t` when the key already works through another authorization source.

Acceptance tests: restricted key with nonzero forced command, agent refusal,
MFA, SFTP failure after authentication, and a working key absent from the `-t`
file. Each must produce the documented outcome without silent permission expansion.

Basis: [SSH exit status](https://man.openbsd.org/ssh#EXIT_STATUS),
[authorized key restrictions](https://man.openbsd.org/sshd#AUTHORIZED_KEYS_FILE_FORMAT),
[AuthenticationMethods](https://man.openbsd.org/sshd_config#AuthenticationMethods).

## P1-3: Interrupted Writes Need an Explicit Partial-Result Contract

Affected sections: Execution Flow; Windows Permissions; SFTP Mode.

Counterexamples: the connection drops halfway through a key; key data is appended
but ACL configuration fails; or the write completes but its acknowledgement is
lost. Blind retries can duplicate entries or append after a partial line.
Automatic rollback can erase a concurrent administrator's update.

Decision needed: distinguish no changes, completed changes, partial changes, and
unknown completion. Specify write ordering, retry rules, and reporting of the
affected path. Do not promise atomicity across append and permission operations.
Define concurrency support for both transports, including newline repair.

Acceptance tests: terminate at each mutation boundary, inject write/close/ACL
failures, and run two installers concurrently. Never report an uncertain update
as clean success or undo unrelated data.

## P1-4: Path Handling and Permission Repair Can Affect Unrelated Data

Affected sections: Remote Operating Systems; Windows Permissions; SFTP Mode.

Counterexamples: `-t` contains quotes or command metacharacters; a destination is
a symlink, Windows reparse point, hard link, directory, or device; its parent is a
shared directory. Applying mode 700 to an arbitrary parent may break other users.

Subsequent decision (2026-09-20): preserve the permissions of the existing parent
directory of a `-t` target in both modes; report insufficient access rather than
changing its mode or ACL. Record this as a difference from upstream SFTP mode,
which sets the parent to mode 700, not as a bug fix. This does not settle key-file,
new-directory, default-path, or link policies. See
[design.md](design.md#recorded-difference-existing-parent-directory-of-a-custom-target).

Remaining decisions: define supported path types, relative-path base, remaining
permission policies, and link handling. Avoid recursive permission changes. A
preflight path check alone does not prevent a concurrent link swap; document the
trust boundary and guarantees actually provided by each transport.

Acceptance tests: paths with quotes, newlines, spaces, non-ASCII characters, and
leading hyphens; existing links and nonregular files; a custom file in a shared
parent directory. Validate local argument handling and remote shell quoting separately.

## P1-5: Windows Administrator Keys Have a Shared Authorization Scope

Affected sections: Project Scope; Remote Operating Systems; Windows Permissions.

The scope says one specified account, but the default Windows administrator
configuration uses a shared `administrators_authorized_keys` file. Installing
there is not necessarily authorization limited to the named account.

Subsequent decision: follow Windows OpenSSH's key-file configuration, display the
destination and shared administrator scope before writing to the shared file,
and document the exception to account-exclusive authorization. Do not change
server configuration to separate accounts. Record this as Windows destination
behavior, not as a Linux compatibility difference. See
[Windows administrator scope](design.md#windows-administrator-scope).

Remaining decisions: define selection of the effective destination and
how custom `AuthorizedKeysFile` settings are handled without pretending the OS
or group membership determines the effective server configuration.
ACL repair must account for existing explicit grants, inheritance, owner, and
deny entries. Granting SYSTEM and Administrators access alone does not establish
that unrelated existing grants have been removed.

Acceptance tests: standard user, administrator, customized server configuration,
unexpected explicit ACL grants, localized group names, and insufficient privileges.

Basis: [Windows OpenSSH key management](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement).

## P2-1: SSH Configuration Needs a Phase-Specific Precedence Policy

Affected sections: Goals; Compatibility; Windows Permissions.

The original design respected SSH configuration but prohibited automatic agent additions.
`AddKeysToAgent=yes` can add a key during an ordinary SSH invocation. Forwarding,
`RemoteCommand`, `RequestTTY`, `SessionType`, and connection multiplexing can also
change the operation. `IdentityAgent` can select an agent different from the one
queried by `ssh-add -L`.

Subsequent decision: do not invoke `ssh-add` to add keys, but respect the user's
`AddKeysToAgent` setting and allow SSH to perform configured additions. Listing
keys with `ssh-add -L` remains in scope; service management remains out of scope.
This preserves upstream behavior and does not add a compatibility difference.
See [agent use and user configuration](design.md#agent-use-and-user-configuration).

Remaining decisions: publish a table of inherited, overridden, and rejected settings
for probing, installation, and SFTP. Preserve needed proxy configuration
while preventing unintended agent forwarding. State the agent-selection contract.

Acceptance tests: conflicting `-o` values and host settings, multiple identity
files, alternate agents, and forwarding configured on a jump host and destination.

Basis: [SSH configuration](https://man.openbsd.org/ssh_config).

## P2-2: Dry Run Cannot Promise That a Connection Has No Side Effects

Affected sections: Compatibility; Initial Validation.

Authentication can generate server logs, invoke login hooks or a forced command,
and prompt for hardware-key confirmation. Local SSH configuration may execute
commands or update known_hosts. Therefore, the current no-modification wording
needs a boundary that the CLI can actually enforce.

Subsequent decision: `-n` connects for installed-key checks unless `-f` skips them,
but performs no destination directory/key-file creation, key append, or permission
changes. Normal connection effects and user-configured `AddKeysToAgent` behavior
remain possible. Help describes skipped installation operations, not absence of
connections or all side effects. This clarifies upstream behavior without adding
a difference. See [dry-run behavior](design.md#dry-run-behavior).

Remaining question: whether `-n -f` avoids connection entirely. Do not silently disable host
key verification to make a dry run noninteractive.

Acceptance tests: first-use host key, login hook, forced command, and `-n -f`.

Basis: [sshd login process](https://man.openbsd.org/sshd#LOGIN_PROCESS).

## P2-3: Public Key Input Needs a Supported Grammar

Affected sections: Execution Flow; Initial Validation.

Counterexamples: a multiline file mixes keys and comments; a key has quoted
`command=` options; a file contains a private key, a BOM, invalid base64, or an
embedded NUL. Treating each nonempty line as a key is insufficient, and rebuilding
an entry from its type and blob can accidentally remove restrictions.

Subsequent decision: reject private key content in installation input before
transmission, including with `-f`, and normalize incoming CRLF line endings to LF.
Preserve key options and comments, existing remote contents, and local source
files. Record these as D-04 and D-05, not bug fixes. See
[public key input](design.md#public-key-input) and the [difference index](compatibility.md).

Remaining decisions: detection method and supported private key formats,
accepted public key formats, multiple-key handling, certificate and CA-entry
scope, size limits, encoding, BOMs, and other malformed input. Strict public key
parsing has not been adopted. Keep the installation entry distinct from the
identity used for a probe.

Acceptance tests: restricted entries, comments and blank lines, CRLF/BOM, malformed
data, explicit certificates, private key input, and oversized input.

## P2-4: Interactive Process Handling Is a Backend Feasibility Gate

Affected sections: Implementation Boundaries; Initial Validation.

Subsequent decision: delegate password and private-key passphrase input to SSH,
and agent confirmation to SSH and the agent. The CLI does not collect or store
those credentials. Preserve upstream behavior without adding a compatibility
difference. Validate concurrent public-key stdin and authentication interaction
with an `ssh.exe` prototype on Windows. See
[authentication interaction](design.md#authentication-interaction).

Prove that public-key stdin and authentication prompts coexist on Windows without
a remote PTY. Test passwords, encrypted private keys, agent confirmation, terminal
absence, stderr/stdout draining, timeouts, and cancellation. Avoid a global
`BatchMode=yes` workaround that disables required first-install authentication.

Specify executable discovery and supported OpenSSH versions. Pair compatible
`ssh`, `ssh-add`, and, if used, `sftp` binaries; Windows OpenSSH, Git, and WSL may
refer to different agents and paths. Snapshot the selected public keys once rather
than repeatedly selecting a possibly changed agent list during installation.

## P2-5: Compatibility Needs an Executable Contract

Affected sections: Compatibility; Initial Validation; Distribution Policy.

Subsequent decision: pin a specific upstream commit as the baseline for argument
parsing, exit codes, and output. Test accepted differences against the project
design; evaluate distribution-specific patches separately. This is a verification
policy, not a new behavioral difference. The selected commit is recorded in
[the pinned reference](design.md#pinned-upstream-reference). A selected-identity
experiment has passed; the full compatibility suite remains unimplemented.

Remaining validation work: cover optional
`-i` arguments, option order, `--`, repeated flags, IPv6, non-UTF-8 Unix paths,
and Windows drive paths. Define exit statuses and stdout/stderr behavior, including
partial outcomes. Verify that Linux packages, Cargo installation, manual pages,
and completions agree on the chosen binary names or document channel differences.

## Suggested Order of Decisions

1. Prototype identity isolation and authentication classification (P1-1, P1-2).
2. Prove interactive process handling on Windows (P2-4).
3. Specify input, path, and partial-write contracts (P1-3, P1-4, P2-3).
4. Resolve SSH configuration precedence and dry-run wording (P2-1, P2-2).
5. Resolve shared administrator scope before implementing Windows destinations (P1-5).
6. Pin and test the compatibility contract before making compatibility claims (P2-5).

Keep the first implementation milestone small. Windows ACL and Linux packaging
decisions can follow their respective milestones, but the Unix installation path
already needs the authentication, input, and failure contracts above.

## Second Round: 2026-09-24

The second round covered the whole design, the pinned upstream script and
manual, the Linux experiment, and fixture results W01 to W05. Its proposals were
decided one at a time on 2026-09-24 and recorded as DL-11 to DL-23 in the
decision log that [design.md](design.md) carried at the time; the design now
states the current decisions without IDs, and the commits that made them name
the IDs. Dispositions of the first round's items:

| Item | Disposition |
| --- | --- |
| P1-1 mechanism | DL-14: three check results; an inconclusive check installs with a warning; configuration generation deferred |
| P1-2 failure classification | DL-12: exit status plus stderr patterns tested per client version; DL-44: untested clients warn and unclassifiable output is Inconclusive |
| P1-3 partial results | DL-13 and DL-32: four outcomes on a result line; `-s` change check; normal-mode concurrent appends still open |
| P1-4 remaining path decisions | DL-17 and DL-28: target paths quoted as data on both destinations; links followed as upstream does |
| P1-5 remaining ACL decisions | DL-19, DL-15, and DL-29: custom `AuthorizedKeysFile` honored only through `-t` |
| P2-1 precedence table | DL-25: upstream's per-phase overrides only; agent selection documented, not corrected |
| P2-2 `-n -f` | DL-22: no connection |
| P2-3 input grammar | DL-27, DL-31: malformed lines rejected, leading byte order mark removed; certificate and CA entries still open |
| P2-4 backend gate | DL-12; the Windows prompt prototype is still required before the first milestone |
| P2-5 compatibility contract | DL-21: golden tests against the pinned script |

Items new in the second round: DL-11 (design stance), DL-15 (D-06), DL-16
(second milestone), DL-18 (destination detection), DL-20 (`-s` on Windows),
and DL-23 (document roles).
