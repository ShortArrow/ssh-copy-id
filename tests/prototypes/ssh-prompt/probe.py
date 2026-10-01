"""A01 prototype: authentication prompts while public keys travel on stdin.

Hosts parent.py in a Windows pseudo console (ConPTY), waits for each prompt
ssh.exe writes to the console, answers it through the console, and checks that
the remote side received the stdin payload byte for byte. Prints JSON results.
Requires the L02 fixture listening on 127.0.0.1 and pywinpty (uv run --with).
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time

from winpty import PtyProcess

HERE = os.path.dirname(os.path.abspath(__file__))
PAYLOAD = b"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA== stdin-probe\nsecond line\r\n"
CR = chr(13)
CTRL_C = chr(3)
REMOTE = "cat > received.tmp && sha256sum < received.tmp | cut -c1-64 && rm received.tmp && id -un"


def ssh_command(args, user, extra):
    return [args.ssh, "-F", "none", "-p", str(args.port),
            "-o", f"UserKnownHostsFile={args.known_hosts}", "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=5", *extra, f"{user}@127.0.0.1", REMOTE]


def run_in_pty(command, steps, timeout=30):
    """Run command in a pseudo console; steps is a list of (prompt regex, keys to send)."""
    proc = PtyProcess.spawn(command, dimensions=(30, 200))
    screen, pending, deadline = "", list(steps), time.time() + timeout
    while time.time() < deadline:
        try:
            chunk = proc.read(4096)
        except EOFError:
            break
        screen += chunk
        if pending and re.search(pending[0][0], screen, re.I):
            proc.write(pending[0][1])
            screen += f"<<sent {pending[0][0]}>>"
            pending.pop(0)
        if not proc.isalive() and not chunk:
            break
    timed_out = proc.isalive()
    if timed_out:
        subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)
    return screen, pending, timed_out


def parent_result(screen):
    for line in reversed(re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", screen).splitlines()):
        line = line.strip()
        if line.startswith('{"exit"'):
            return json.loads(line)
    return None


def case(name, command, steps, expect_exit, expect_user=None):
    screen, pending, timed_out = run_in_pty(command, steps)
    result = parent_result(screen)
    expected_hash = hashlib.sha256(PAYLOAD).hexdigest()
    lines = (result or {}).get("stdout", "").split()
    passed = (not timed_out and not pending and result is not None and result["exit"] == expect_exit
              and (expect_user is None or lines == [expected_hash, expect_user]))
    return {"case": name, "passed": passed, "timed_out": timed_out,
            "unanswered_prompts": [p for p, _ in pending], "parent": result,
            "console_tail": re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", screen)[-300:]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ssh", default=r"C:\Windows\System32\OpenSSH\ssh.exe")
    parser.add_argument("--port", type=int, default=22022)
    parser.add_argument("--known-hosts", required=True)
    parser.add_argument("--key", required=True, help="private key with passphrase, authorized for keyuser")
    parser.add_argument("--passphrase", required=True)
    parser.add_argument("--password", required=True, help="the PWUSER_PASSWORD the fixture was started with")
    parser.add_argument("--payload", required=True, help="file to write the stdin payload to")
    args = parser.parse_args()
    with open(args.payload, "wb") as out:
        out.write(PAYLOAD)
    parent = [sys.executable, os.path.join(HERE, "parent.py"), args.payload]
    password_only = ["-o", "PubkeyAuthentication=no", "-o", "PreferredAuthentications=password",
                     "-o", "NumberOfPasswordPrompts=1", "-o", "IdentityAgent=none"]
    key_only = ["-i", args.key, "-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none",
                "-o", "PreferredAuthentications=publickey"]
    cases = [
        ("password prompt answered through the console", "pwuser", password_only,
         [(r"password:", args.password + CR)], 0, "pwuser"),
        ("passphrase prompt answered through the console", "keyuser", key_only,
         [(r"passphrase for key", args.passphrase + CR)], 0, "keyuser"),
        ("cancellation with Ctrl-C at the password prompt", "pwuser", password_only,
         [(r"password:", CTRL_C)], 255, None),
    ]
    for name, user, extra, steps, expect_exit, expect_user in cases:
        print(json.dumps(case(name, parent + ssh_command(args, user, extra), steps, expect_exit, expect_user),
                         ensure_ascii=False), flush=True)
    print(json.dumps(no_terminal(parent + ssh_command(args, "pwuser", password_only)), ensure_ascii=False), flush=True)
    batch = no_terminal(parent + ssh_command(args, "pwuser", password_only + ["-o", "BatchMode=yes"]))
    batch["case"] = "no terminal: password authentication with BatchMode=yes"
    print(json.dumps(batch, ensure_ascii=False), flush=True)


def no_terminal(command, timeout=20):
    """Run the parent with no console at all and report whether ssh fails, succeeds, or waits."""
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             creationflags=subprocess.CREATE_NO_WINDOW | subprocess.DETACHED_PROCESS)
    started = time.time()
    try:
        stdout, stderr = child.communicate(timeout=timeout)
        outcome = "exited"
    except subprocess.TimeoutExpired:
        subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], capture_output=True)
        stdout, stderr = child.communicate()
        outcome = f"still waiting after {timeout} s; process tree killed"
    last = stdout.decode("utf-8", "replace").strip().splitlines()
    return {"case": "no terminal: password authentication without a console", "outcome": outcome,
            "seconds": round(time.time() - started, 1),
            "parent": json.loads(last[-1]) if last and last[-1].startswith("{") else None,
            "stderr": stderr.decode("utf-8", "replace")[-300:]}


if __name__ == "__main__":
    main()
