#!/bin/sh
# Builds and starts the Linux destination fixture (L02) and checks key-only and
# password logins. Requires Docker and an OpenSSH client with SSH_ASKPASS_REQUIRE.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
name="ssh-copy-id-l02-$$"
port="${L02_PORT:-22022}"
cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf "$work"; }
trap cleanup EXIT INT TERM

docker build -q -t ssh-copy-id-l02:local "$here" >/dev/null
ssh-keygen -q -t ed25519 -N '' -f "$work/control"
password=$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')
docker run -d --name "$name" -p "127.0.0.1:$port:22" -e PWUSER_PASSWORD="$password" \
    -e CONTROL_PUBLIC_KEY="$(cat "$work/control.pub")" ssh-copy-id-l02:local >/dev/null

opts="-F none -o UserKnownHostsFile=$work/known_hosts -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 -p $port"
i=0
until ssh $opts -o BatchMode=yes -o IdentitiesOnly=yes -i "$work/control" keyuser@127.0.0.1 true 2>/dev/null; do
    i=$((i + 1))
    if [ "$i" -ge 30 ]; then docker logs "$name" >&2; echo "fixture did not accept the control key" >&2; exit 1; fi
    sleep 1
done
echo "pass: keyuser logs in with the control key"

[ "$(ssh $opts -o BatchMode=yes -o IdentitiesOnly=yes -i "$work/control" keyuser@127.0.0.1 id -un)" = keyuser ]
echo "pass: remote command runs as keyuser"

printf '#!/bin/sh\necho %s\n' "$password" > "$work/askpass"
chmod 700 "$work/askpass"
[ "$(SSH_ASKPASS="$work/askpass" SSH_ASKPASS_REQUIRE=force ssh $opts -o PubkeyAuthentication=no \
    -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1 pwuser@127.0.0.1 id -un </dev/null)" = pwuser ]
echo "pass: pwuser logs in with the password"

if SSH_ASKPASS="$work/askpass" SSH_ASKPASS_REQUIRE=force ssh $opts -o PubkeyAuthentication=no \
    -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1 keyuser@127.0.0.1 true </dev/null 2>/dev/null; then
    echo "keyuser accepted a password" >&2; exit 1
fi
echo "pass: keyuser rejects password authentication"
