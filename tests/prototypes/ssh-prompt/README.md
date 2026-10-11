# Authentication Prompt Experiment (A01)

Checks whether `ssh` on Windows can prompt for a password or passphrase on the
console while the public key data travels on its stdin pipe, which is how the
CLI will run it. `parent.py` stands in for the CLI: it starts `ssh` with stdin
and stdout piped and the console inherited. `probe.py` runs `parent.py` inside
a Windows pseudo console (ConPTY), waits for each prompt, answers it through the
console, and checks that the remote side received the stdin payload byte for
byte. It also runs the parent with no console at all.

Start the [Linux fixture](../../environments/linux/) with a passphrase-protected
control key, then run the probe from the repository root (PowerShell):

```powershell
ssh-keygen -q -t ed25519 -N 'pass phrase 1' -f work/a01/enc_key
$password = [Guid]::NewGuid().ToString('N')
docker build -t ssh-copy-id-l02:local tests/environments/linux
docker run -d --name ssh-copy-id-a01 -p 127.0.0.1:22022:22 -e "PWUSER_PASSWORD=$password" `
  -e "CONTROL_PUBLIC_KEY=$((Get-Content work/a01/enc_key.pub -Raw).Trim())" ssh-copy-id-l02:local
uv run --no-project --with pywinpty python tests/prototypes/ssh-prompt/probe.py --password $password `
  --known-hosts work/a01/known_hosts --key work/a01/enc_key --passphrase 'pass phrase 1' --payload work/a01/payload.bin
docker rm -f ssh-copy-id-a01
```

`--ssh` selects the client; the default is `C:\Windows\System32\OpenSSH\ssh.exe`.
pywinpty is test tooling fetched by `uv` into its cache, not a project
dependency. The probe prints one JSON line per case.

`agent_confirm.py` covers agent confirmation. It changes the user's agent, so
run it only with a disposable key, and remove the key afterwards: add the key
with `ssh-add -c`, move the private key file away so `ssh` can use the key only
through the agent, run the script with `--identity` set to the private key path,
then `ssh-add -d` with the public key and compare `ssh-add -l` with its output
before the experiment. The Windows OpenSSH agent refuses `ssh-add -c`, so with
that agent the script observes a public-key rejection.

`cli_probe.py` runs the same prompts through `ssh-copy-id.exe` itself, which
gives `ssh` files instead of pipes as its standard streams. It installs four
new keys: one after a password prompt, one after the passphrase prompt of the
encrypted control key, one with Ctrl-C at the password prompt, and one with no
console. Put the client to test first on `PATH`, start the fixture as above,
and run:

```powershell
cargo build --release
foreach ($i in 1..4) { ssh-keygen -q -t ed25519 -N '' -C "a01-k$i" -f "work/a01/k$i" }
uv run --no-project --with pywinpty python tests/prototypes/ssh-prompt/cli_probe.py `
  --cli target/release/ssh-copy-id.exe --ssh (Get-Command ssh).Source --port 22022 `
  --known-hosts work/a01/known_hosts --control-key work/a01/enc_key --passphrase 'pass phrase 1' `
  --password $password --keys work/a01/k1 work/a01/k2 work/a01/k3 work/a01/k4
```
