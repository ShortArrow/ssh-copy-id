"""Disposable localhost-only experiment; not a production SSH config resolver."""

import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile
import time


def run(args, expected=None):
    result = subprocess.run(args, capture_output=True, text=True, timeout=30)
    if expected is not None and result.returncode != expected:
        raise AssertionError(f"{args}: exit {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result


def key(path):
    run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(path)], 0)
    return Path(str(path) + ".pub").read_text()


def main():
    with tempfile.TemporaryDirectory(prefix="identity-probe-", dir="/home/fixture") as scratch:
        root = Path(scratch)
        root.chmod(0o755)
        os.environ["HOME"] = str(root)
        (root / ".ssh").mkdir(mode=0o700)
        a, b, jump_key = [root / name for name in ("key_a", "key_b", "jump_key")]
        pub_a, pub_b, pub_jump = [key(path) for path in (a, b, jump_key)]
        host = root / "host_key"
        pub_host = key(host)
        target_auth, jump_auth = root / "target_authorized", root / "jump_authorized"
        target_auth.write_text(pub_a)
        jump_auth.write_text(pub_jump)
        known = root / "known_hosts"
        known.write_text("".join(f"[127.0.0.1]:{port} {pub_host}" for port in (2222, 2223)))
        daemons = []
        try:
            for port, auth in ((2222, target_auth), (2223, jump_auth)):
                config = root / f"sshd_{port}.conf"
                config.write_text(f"""Port {port}
ListenAddress 127.0.0.1
HostKey {host}
PidFile {root}/sshd_{port}.pid
AuthorizedKeysFile {auth}
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitRootLogin no
AllowUsers fixture
AllowTcpForwarding yes
LogLevel VERBOSE
Subsystem sftp internal-sftp
""")
                run(["/usr/sbin/sshd", "-t", "-f", str(config)], 0)
                daemons.append(subprocess.Popen([
                    "/usr/sbin/sshd", "-D", "-f", str(config), "-E", str(root / f"sshd_{port}.log")
                ]))
                for attempt in range(100):
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                            break
                    except OSError:
                        time.sleep(0.05)
                else:
                    raise RuntimeError("sshd did not start")

            original = root / "client.conf"
            original.write_text(f"""Host target via-jump
    HostName 127.0.0.1
    Port 2222
    User fixture
    IdentityFile {a}
Host via-jump
    ProxyJump jump
Host jump
    HostName 127.0.0.1
    Port 2223
    User fixture
    IdentityFile {jump_key}
Host *
    UserKnownHostsFile {known}
    GlobalKnownHostsFile /dev/null
    StrictHostKeyChecking yes
    BatchMode yes
    IdentitiesOnly yes
""")
            results = []

            def record(name, condition, **data):
                results.append({"case": name, "passed": bool(condition), **data})
                if not condition:
                    raise AssertionError(json.dumps(results, indent=2))

            def probe(config, dest, identity=b):
                return run(["ssh", "-F", str(config), "-i", str(identity),
                            "-o", "IdentitiesOnly=yes", "-o", "ControlPath=none",
                            "-o", "PreferredAuthentications=publickey", dest, "exit"])

            def isolated(dest):
                effective = run(["ssh", "-G", "-F", str(original), dest], 0).stdout
                # Preserve effective settings, replacing the additive identity lists.
                # Re-evaluate the jump's settings in a separate SSH process using the
                # original config. This fixture supports exactly one fixed jump alias.
                omitted = {"identityfile", "certificatefile", "identitiesonly", "controlpath",
                           "controlmaster", "controlpersist", "proxyjump", "proxycommand",
                           "preferredauthentications", "identityagent"}
                lines = [line for line in effective.splitlines()
                         if line.split()[0] not in omitted]
                lines += [f"IdentityFile {b}", "CertificateFile none", "IdentityAgent none",
                          "IdentitiesOnly yes", "ControlPath none", "ControlMaster no",
                          "ControlPersist no", "PreferredAuthentications publickey"]
                if dest == "via-jump":
                    lines.append("ProxyCommand " + shlex.join([
                        "ssh", "-F", str(original), "-W", "%h:%p", "jump"]))
                path = root / f"isolated-{dest}.conf"
                path.write_text("\n".join(lines) + "\n")
                return path

            version = run(["ssh", "-V"]).stderr.strip()
            print(json.dumps({"ssh_version": version, "upstream_commit":
                              "eabf1987de772f0f2d772fd6bd72b4c84d0ab780",
                              "upstream_sha256": hashlib.sha256(
                                  Path('/opt/upstream-ssh-copy-id').read_bytes()).hexdigest()}), flush=True)
            for dest in ("target", "via-jump"):
                target_auth.write_text(pub_a)
                ordinary = probe(original, dest)
                record(f"{dest}: -i B still succeeds through configured A", ordinary.returncode == 0,
                       stderr=ordinary.stderr, server_log=(root / "sshd_2222.log").read_text()
                       if ordinary.returncode else "")
                upstream = run(["sh", "/opt/upstream-ssh-copy-id", "-i", str(b) + ".pub",
                                "-F", str(original), dest])
                record(f"{dest}: upstream skips B", upstream.returncode == 0
                       and "All keys were skipped" in upstream.stdout + upstream.stderr
                       and target_auth.read_text() == pub_a,
                       exit_code=upstream.returncode)
                config = isolated(dest)
                identities = [line for line in run(["ssh", "-G", "-F", str(config), dest], 0)
                              .stdout.splitlines() if line.startswith("identityfile ")]
                record(f"{dest}: effective config contains only B", identities == [f"identityfile {b}"])
                denied = probe(config, dest)
                record(f"{dest}: isolated B is rejected when absent", denied.returncode == 255
                       and "Permission denied" in denied.stderr)
                target_auth.write_text(pub_a + pub_b)
                record(f"{dest}: isolated B succeeds when installed", probe(config, dest).returncode == 0)
                # Preserve trust checking even with a separately generated config.
                saved = known.read_text()
                known.write_text(f"[127.0.0.1]:2223 {pub_host}")
                refused = probe(config, dest)
                known.write_text(saved)
                record(f"{dest}: missing trusted host key blocks connection", refused.returncode == 255
                       and "Host key verification failed" in refused.stderr)

            target_log = (root / "sshd_2222.log").read_text()
            for label, path in (("A", a), ("B", b)):
                fingerprint = run(["ssh-keygen", "-lf", str(path) + ".pub"], 0).stdout.split()[1]
                record(f"server confirms successful authentication with {label}",
                       any("Accepted publickey" in line and fingerprint in line
                           for line in target_log.splitlines()), fingerprint=fingerprint)
            print(json.dumps({"results": results, "passed": len(results)}, indent=2), flush=True)
        finally:
            for daemon in daemons:
                daemon.terminate()
            for daemon in daemons:
                daemon.wait(timeout=5)


if __name__ == "__main__":
    main()
