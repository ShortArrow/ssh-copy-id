#!/bin/sh
# Sets root's password and authorizes the control key in Dropbear's root key
# file from the environment, then runs Dropbear in the foreground. The caller
# generates both for each run. Both are required: the image's root has an empty
# password, and Dropbear lets an empty password in without any key.
set -eu
if [ -z "${ROOT_PASSWORD:-}" ]; then
    echo "ROOT_PASSWORD must be set" >&2; exit 1
fi
case "${CONTROL_PUBLIC_KEY:-}" in
    ssh-ed25519\ *) ;;
    *) echo "CONTROL_PUBLIC_KEY must be an ssh-ed25519 public key" >&2; exit 1 ;;
esac
printf '%s\n%s\n' "$ROOT_PASSWORD" "$ROOT_PASSWORD" | passwd root >/dev/null 2>&1
printf '%s\n' "$CONTROL_PUBLIC_KEY" > /etc/dropbear/authorized_keys
chmod 600 /etc/dropbear/authorized_keys
key=/etc/dropbear/dropbear_ed25519_host_key
[ -f "$key" ] || dropbearkey -t ed25519 -f "$key" >/dev/null 2>&1
dropbearkey -y -f "$key" | grep Fingerprint
exec /usr/sbin/dropbear -F -E -p 22 -r "$key"
