"""Stand-in for the CLI: runs ssh with public key data on a stdin pipe.

The child inherits this process's console for stderr and for any prompt that
reads the console directly, as the Rust CLI's child would. Prints the child's
exit status and stdout as JSON on the last line.
"""
import json
import subprocess
import sys


def main() -> None:
    payload_path, *ssh_command = sys.argv[1:]
    with open(payload_path, "rb") as source:
        payload = source.read()
    child = subprocess.Popen(ssh_command, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    stdout, _ = child.communicate(payload)
    print(json.dumps({"exit": child.returncode, "stdout": stdout.decode("utf-8", "replace")}), flush=True)


if __name__ == "__main__":
    main()
