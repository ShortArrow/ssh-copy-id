#!/bin/sh
# Validation row W09, Unix side: runs each candidate shell-family probe as the
# SSH command of an account whose login shell is sh, dash, bash, or tcsh, with
# and without a startup file that prints a line, and prints one line per run:
# shell|startup|candidate|exit status|stdout as `sed -n l` shows it.
# When /pwsh/pwsh.tar.gz exists (a PowerShell Linux release archive), pwsh is
# extracted and also used as a login shell.
# Runs inside a throwaway container of the L02 image:
#   docker run --rm --entrypoint sh -v "$PWD/tests/environments/linux:/probe:ro" \
#     [-v <dir with pwsh.tar.gz>:/pwsh:ro] ssh-copy-id-l02:local /probe/probe-shells.sh
set -eu
ssh-keygen -q -t ed25519 -N '' -f /root/probe_key
install -d -m 700 -o keyuser -g keyuser /home/keyuser/.ssh
install -m 600 -o keyuser -g keyuser /root/probe_key.pub /home/keyuser/.ssh/authorized_keys
/usr/sbin/sshd
shells='/bin/sh /bin/dash /bin/bash /usr/bin/tcsh'
if [ -f /pwsh/pwsh.tar.gz ]; then
    apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends libicu74 >/dev/null
    mkdir -p /opt/pwsh && tar -xzf /pwsh/pwsh.tar.gz -C /opt/pwsh && chmod +x /opt/pwsh/pwsh
    echo /opt/pwsh/pwsh >>/etc/shells
    shells="$shells /opt/pwsh/pwsh"
fi

probe() {
    status=0
    out=$(ssh -F none -T -i /root/probe_key -o IdentitiesOnly=yes -o BatchMode=yes \
        -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/known_hosts keyuser@127.0.0.1 "$2" 2>/dev/null) || status=$?
    printf '%s|%s|%s|%s|' "$3" "$4" "$1" "$status"
    printf '%s\n' "$out" | sed -n l | tr '\n' ' '
    printf '\n'
}

for shell in $shells; do
    usermod -s "$shell" keyuser
    for startup in none prints; do
        rm -f /home/keyuser/.bashrc /home/keyuser/.cshrc /home/keyuser/.config/powershell/Microsoft.PowerShell_profile.ps1
        if [ "$startup" = prints ]; then
            case "$shell" in
                /bin/sh | /bin/dash) continue ;;
                /bin/bash) echo 'echo startup-line' >/home/keyuser/.bashrc ;;
                /usr/bin/tcsh) echo 'echo startup-line' >/home/keyuser/.cshrc ;;
                /opt/pwsh/pwsh)
                    install -d -o keyuser -g keyuser /home/keyuser/.config /home/keyuser/.config/powershell
                    echo "'startup-line'" >/home/keyuser/.config/powershell/Microsoft.PowerShell_profile.ps1 ;;
            esac
        fi
        probe B 'echo ssh-copy-id-probe:%OS%:$0' "$shell" "$startup"
        probe C 'echo ssh-copy-id-probe:%OS%:$0/$home' "$shell" "$startup"
    done
done
