#!/bin/sh
# Sets the password of pwuser, cshuser, and mfauser and keyuser's control key
# from the environment, then runs sshd in the foreground. The caller generates
# both for each run.
set -eu
if [ -n "${PWUSER_PASSWORD:-}" ]; then
    for user in pwuser cshuser mfauser; do
        printf '%s:%s\n' "$user" "$PWUSER_PASSWORD" | chpasswd
        passwd -u "$user" >/dev/null
    done
fi
if [ -n "${CONTROL_PUBLIC_KEY:-}" ]; then
    case "$CONTROL_PUBLIC_KEY" in
        ssh-ed25519\ *) ;;
        *) echo "CONTROL_PUBLIC_KEY must be an ssh-ed25519 public key" >&2; exit 1 ;;
    esac
    install -d -m 700 -o keyuser -g keyuser /home/keyuser/.ssh
    printf '%s\n' "$CONTROL_PUBLIC_KEY" > /home/keyuser/.ssh/authorized_keys
    chown keyuser:keyuser /home/keyuser/.ssh/authorized_keys
    chmod 600 /home/keyuser/.ssh/authorized_keys
fi
for key in /etc/ssh/ssh_host_*_key.pub; do ssh-keygen -lf "$key"; done
exec /usr/sbin/sshd -D -e
