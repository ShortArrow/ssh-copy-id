"""A01 agent-confirmation case: what ssh does when the agent key requires confirmation.

Runs parent.py in a ConPTY with ssh using only the agent key whose public half
is given, records the console and the result, and answers nothing. Adding the
key with `ssh-add -c` and removing it afterwards is the caller's job.
"""
import argparse
import json
import os
import re
import sys
import time

from winpty import PtyProcess

HERE = os.path.dirname(os.path.abspath(__file__))
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ssh", default=r"C:\Windows\System32\OpenSSH\ssh.exe")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--known-hosts", required=True)
    parser.add_argument("--identity", required=True, help="private key file whose key is in the agent")
    parser.add_argument("--payload", required=True)
    parser.add_argument("--timeout", type=int, default=30)
    args = parser.parse_args()
    command = [sys.executable, os.path.join(HERE, "parent.py"), args.payload, args.ssh, "-F", "none",
               "-p", str(args.port), "-o", f"UserKnownHostsFile={args.known_hosts}",
               "-o", "StrictHostKeyChecking=accept-new", "-o", "ConnectTimeout=5",
               "-i", args.identity, "-o", "IdentitiesOnly=yes", "-o", "PreferredAuthentications=publickey",
               "keyuser@127.0.0.1", "id -un"]
    proc = PtyProcess.spawn(command, dimensions=(30, 200))
    screen, started = "", time.time()
    while time.time() - started < args.timeout:
        try:
            screen += proc.read(4096)
        except EOFError:
            break
        if not proc.isalive():
            break
    alive = proc.isalive()
    if alive:
        os.system(f"taskkill /PID {proc.pid} /T /F >NUL 2>&1")
    print(json.dumps({"case": "agent key added with ssh-add -c", "ssh": args.ssh,
                      "outcome": f"still waiting after {args.timeout} s; killed" if alive else "exited",
                      "seconds": round(time.time() - started, 1),
                      "console": ANSI.sub("", screen)[-500:]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
