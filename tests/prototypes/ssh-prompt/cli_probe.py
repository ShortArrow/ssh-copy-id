"""A01 with the CLI: prompts while ssh-copy-id runs ssh with its streams on files.

Runs ssh-copy-id.exe in a Windows pseudo console (ConPTY), answers each prompt
through the console, and checks the result by logging in with the new key.
Prints one JSON line per case.
"""
import argparse
import json
import re
import subprocess
import time

from winpty import PtyProcess

CR = chr(13)
CTRL_C = chr(3)
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07")


def run_in_pty(command, prompt, answer, timeout=60):
    """Answers every new occurrence of `prompt`; returns screen, answers, exit, timed_out."""
    proc = PtyProcess.spawn(command, dimensions=(30, 200))
    screen, answered, deadline = "", 0, time.time() + timeout
    while time.time() < deadline:
        try:
            chunk = proc.read(4096)
        except EOFError:
            break
        screen += chunk
        if prompt and len(re.findall(prompt, ANSI.sub("", screen), re.I)) > answered:
            proc.write(answer)
            answered += 1
        if not proc.isalive() and not chunk:
            break
    timed_out = proc.isalive()
    if timed_out:
        subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)
    return ANSI.sub("", screen), answered, proc.exitstatus, timed_out


def logs_in(args, key, user):
    result = subprocess.run(base_ssh(args, user) + ["-i", key, "-o", "IdentitiesOnly=yes",
                                                    "-o", "PreferredAuthentications=publickey",
                                                    "-o", "BatchMode=yes", f"{user}@127.0.0.1", "true"],
                            capture_output=True, timeout=30)
    return result.returncode == 0


def base_ssh(args, user):
    return [args.ssh, "-F", "none", "-p", str(args.port), "-o", f"UserKnownHostsFile={args.known_hosts}",
            "-o", "StrictHostKeyChecking=accept-new", "-o", "IdentityAgent=none"]


def cli(args, key, user, extra):
    return [args.cli, "-i", key, "-p", str(args.port), "-F", "none",
            "-o", f"UserKnownHostsFile={args.known_hosts}", "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=5", "-o", "IdentityAgent=none", *extra, f"{user}@127.0.0.1"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", required=True)
    parser.add_argument("--ssh", required=True, help="the client first on PATH, for the login check")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--known-hosts", required=True)
    parser.add_argument("--control-key", required=True)
    parser.add_argument("--passphrase", required=True)
    parser.add_argument("--password", required=True)
    parser.add_argument("--keys", nargs=4, required=True, help="four new private keys")
    args = parser.parse_args()
    password_only = ["-o", "PubkeyAuthentication=no", "-o", "PreferredAuthentications=password,publickey"]
    k1, k2, k3, k4 = args.keys
    results = []

    screen, answered, status, timed_out = run_in_pty(cli(args, k1, "pwuser", ["-o", "IdentitiesOnly=yes"]), r"password:", args.password + CR)
    results.append({"case": "password prompt", "passed": status == 0 and not timed_out and answered >= 1
                    and logs_in(args, k1, "pwuser"), "answers": answered, "exit": status, "tail": screen[-400:]})

    screen, answered, status, timed_out = run_in_pty(
        cli(args, k2, "keyuser", ["-o", f"IdentityFile={args.control_key}", "-o", "IdentitiesOnly=yes"]),
        r"passphrase for key", args.passphrase + CR)
    results.append({"case": "passphrase prompt", "passed": status == 0 and not timed_out and answered >= 1
                    and logs_in(args, k2, "keyuser"), "answers": answered, "exit": status, "tail": screen[-400:]})

    screen, answered, status, timed_out = run_in_pty(cli(args, k3, "pwuser", password_only), r"password:", CTRL_C)
    results.append({"case": "Ctrl-C at the password prompt", "passed": status == 1 and not timed_out
                    and answered == 1 and not logs_in(args, k3, "pwuser"), "answers": answered, "exit": status,
                    "tail": screen[-400:]})

    started = time.time()
    child = subprocess.Popen(cli(args, k4, "pwuser", password_only), stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             creationflags=subprocess.CREATE_NO_WINDOW | subprocess.DETACHED_PROCESS)
    try:
        out, err = child.communicate(timeout=30)
        status = child.returncode
    except subprocess.TimeoutExpired:
        subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], capture_output=True)
        out, err = child.communicate()
        status = None
    results.append({"case": "no console, password authentication", "passed": status == 1
                    and not logs_in(args, k4, "pwuser"), "exit": status,
                    "seconds": round(time.time() - started, 1), "stderr": err.decode("utf-8", "replace")[-400:]})

    for result in results:
        print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
