#!/bin/sh
# Builds and starts the OpenWrt destination fixture (L03) and checks the root
# logins and the properties of stock OpenWrt the tests rely on. Requires Docker
# and an OpenSSH client with SSH_ASKPASS_REQUIRE.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
name="ssh-copy-id-l03-$$"
port="${L03_PORT:-22023}"
cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf "$work"; }
trap cleanup EXIT INT TERM

docker build -q -t ssh-copy-id-l03:local "$here" >/dev/null
ssh-keygen -q -t ed25519 -N '' -f "$work/control"
ssh-keygen -q -t ed25519 -N '' -f "$work/other"
password=$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')
docker run -d --name "$name" -p "127.0.0.1:$port:22" -e ROOT_PASSWORD="$password" \
    -e CONTROL_PUBLIC_KEY="$(cat "$work/control.pub")" ssh-copy-id-l03:local >/dev/null

opts="-F none -o UserKnownHostsFile=$work/known_hosts -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 -p $port"
key_login() { ssh $opts -o BatchMode=yes -o IdentitiesOnly=yes -i "$1" root@127.0.0.1 "$2" </dev/null; }
i=0
until key_login "$work/control" true 2>/dev/null; do
    i=$((i + 1))
    if [ "$i" -ge 30 ]; then docker logs "$name" >&2; echo "fixture did not accept the control key" >&2; exit 1; fi
    sleep 1
done
echo "pass: root logs in with the control key"

key_login "$work/control" 'cat /etc/openwrt_release; dropbear -V; ls --help 2>&1 | head -1' 2>&1 | sed 's/^/  /'
[ "$(key_login "$work/control" 'id -u')" = 0 ]
echo "pass: remote command runs as root"

printf '#!/bin/sh\necho %s\n' "$password" > "$work/askpass"
chmod 700 "$work/askpass"
[ "$(SSH_ASKPASS="$work/askpass" SSH_ASKPASS_REQUIRE=force ssh $opts -o PubkeyAuthentication=no \
    -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1 root@127.0.0.1 id -u </dev/null)" = 0 ]
echo "pass: root logs in with the password"

if key_login "$work/other" true 2>/dev/null; then
    echo "root accepted a key that is not authorized" >&2; exit 1
fi
echo "pass: root rejects a key that is not authorized"

key_login "$work/control" "mkdir -p -m 700 /root/.ssh && echo '$(cat "$work/other.pub")' > /root/.ssh/authorized_keys"
if key_login "$work/other" true 2>/dev/null; then
    echo "Dropbear read /root/.ssh/authorized_keys for root" >&2; exit 1
fi
key_login "$work/control" 'rm -r /root/.ssh'
echo "pass: Dropbear reads root's keys only from /etc/dropbear/authorized_keys"

if key_login "$work/control" 'command -v od' >/dev/null; then
    echo "the fixture has od, which stock OpenWrt lacks" >&2; exit 1
fi
echo "pass: BusyBox has no od, as on stock OpenWrt"
