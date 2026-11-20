#!/bin/bash
# A fixed signing identity for builds on this Mac, so macOS keeps Accessibility and Screen
# Recording across rebuilds. Ad-hoc signatures change with every build, and macOS then treats
# Shifu and Shifu Computer Use as new apps that need permission again.
#   bash scripts/local-signing.sh create   make "Shifu Local Signing" in the login keychain (once)
#   bash scripts/local-signing.sh name     print the identity to sign with, or "-" (ad-hoc) if none
# A real Developer ID (SHIFU_SIGNING_IDENTITY / APPLE_SIGNING_IDENTITY) still wins over this.
set -euo pipefail
NAME="Shifu Local Signing"
case "${1:-name}" in
  name)
    if /usr/bin/security find-identity -p codesigning 2>/dev/null | grep -q "\"$NAME\""; then echo "$NAME"; else echo "-"; fi ;;
  create)
    if /usr/bin/security find-identity -p codesigning 2>/dev/null | grep -q "\"$NAME\""; then echo "已经有 $NAME"; exit 0; fi
    dir=$(mktemp -d); trap 'rm -rf "$dir"' EXIT
    cat > "$dir/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
    openssl req -x509 -newkey rsa:2048 -nodes -keyout "$dir/key.pem" -out "$dir/cert.pem" -days 3650 -config "$dir/cert.cnf" 2>/dev/null
    pass=$(openssl rand -hex 16)
    openssl pkcs12 -export -legacy -inkey "$dir/key.pem" -in "$dir/cert.pem" -name "$NAME" -out "$dir/id.p12" -passout "pass:$pass"
    /usr/bin/security import "$dir/id.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$pass" -T /usr/bin/codesign
    echo "已建好 $NAME。重新构建 Shifu 和 Shifu Computer Use 后，在系统设置里再授权一次，之后重建不会再丢权限。" ;;
  *) echo "用法：bash scripts/local-signing.sh [name|create]" >&2; exit 2 ;;
esac
