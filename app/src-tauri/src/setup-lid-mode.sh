#!/bin/zsh
# Adapted from Steamcup, MIT, Copyright (c) 2026 Yutai Lin.
# One-time setup for Shifu's lid-closed mode. Run:  sudo zsh setup-lid-mode.sh
# (Shifu itself runs it as root without sudo and passes your user name as the first argument.)
# Lets your user run exactly two commands without a password — nothing else:
#   pmset -a disablesleep 1   (lid closed → keep running)
#   pmset -a disablesleep 0   (back to normal)
# Undo:  sudo pmset -a disablesleep 0 && sudo rm /etc/sudoers.d/shifu-steamcup
set -e
export PATH=/usr/bin:/bin:/usr/sbin:/sbin   # runs as root: only system tools, never ones from the user's PATH
umask 077
[[ $EUID -eq 0 ]] || { echo "Run with sudo:  sudo zsh $0"; exit 1; }
U=${SUDO_USER:-${1:-}}
# Must be a real login account (uid 500–99999), not root, a system account or "nobody".
[[ $U =~ '^[A-Za-z0-9._-]+$' ]] && UID_=$(id -u -- "$U" 2>/dev/null || echo 0) && (( UID_ >= 500 && UID_ < 100000 )) \
  || { echo "Run with sudo from your own account, or pass your user name:  sudo zsh $0 <user>"; exit 1; }
# Built inside the root-only folder (sudo ignores names starting with "."), checked, then renamed into place in one step.
TMP=$(mktemp /etc/sudoers.d/.shifu-steamcup.XXXXXX)
trap 'rm -f "$TMP"' EXIT
echo "$U ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0" > "$TMP"
visudo -cf "$TMP" >/dev/null || { echo "Rule failed the sudoers check — nothing installed."; exit 1; }
chown root:wheel "$TMP"
chmod 440 "$TMP"
mv -f "$TMP" /etc/sudoers.d/shifu-steamcup
echo "Done. Shifu can now keep your Mac running with the lid closed (for user $U)."
