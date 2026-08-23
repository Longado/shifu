#!/usr/bin/env bash
# Install SHIFU's Codex adapter skills into ~/.codex/skills.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SOURCE_DIR="${ADAPTER_SOURCE_DIR:-${REPO_ROOT}/codex/skills}"
TARGET_DIR="${CODEX_HOME:-${HOME}/.codex}/skills"
MANAGED_SKILLS=(shifu shifu-research shifu-review)

check_mode=false
if [ "${1:-}" = "--check" ]; then
  check_mode=true
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--check]" >&2
  exit 2
fi

for skill in "${MANAGED_SKILLS[@]}"; do
  source_skill="${SOURCE_DIR}/${skill}"
  skill_file="${source_skill}/SKILL.md"
  if [ ! -s "$skill_file" ] \
    || [ "$(sed -n '1p' "$skill_file")" != "---" ] \
    || ! grep -qx "name: ${skill}" "$skill_file"; then
    echo "invalid adapter source: ${source_skill}/SKILL.md" >&2
    exit 1
  fi
done

has_unknown_target_files() {
  local source_skill="$1" target_skill="$2" path rel
  [ -d "$target_skill" ] || return 1
  while IFS= read -r -d '' path; do
    rel="${path#${target_skill}/}"
    source_path="${source_skill}/${rel}"
    if { [ -d "$path" ] && [ ! -L "$path" ] && [ ! -d "$source_path" ]; } \
      || { { [ ! -d "$path" ] || [ -L "$path" ]; } \
        && [ ! -e "$source_path" ] && [ ! -L "$source_path" ]; }; then
      echo "refusing to overwrite unmanaged path: ${path}" >&2
      return 0
    fi
  done < <(find "$target_skill" -mindepth 1 -print0)
  return 1
}

for skill in "${MANAGED_SKILLS[@]}"; do
  target_skill="${TARGET_DIR}/${skill}"
  if [ -e "$target_skill" ] && [ ! -d "$target_skill" ]; then
    echo "refusing to overwrite non-directory target: ${target_skill}" >&2
    exit 1
  fi
  if has_unknown_target_files "${SOURCE_DIR}/${skill}" "$target_skill"; then
    exit 1
  fi
done

if [ "$check_mode" = true ]; then
  drift=0
  for skill in "${MANAGED_SKILLS[@]}"; do
    if [ ! -d "${TARGET_DIR}/${skill}" ] \
      || ! diff -qr "${SOURCE_DIR}/${skill}" "${TARGET_DIR}/${skill}" >/dev/null; then
      echo "drift: ${skill}" >&2
      drift=1
    else
      echo "ok: ${skill}"
    fi
  done
  exit "$drift"
fi

mkdir -p "$TARGET_DIR"
staging="$(mktemp -d "${TARGET_DIR}/.shifu-install.XXXXXX")"
mkdir -p "${staging}/new" "${staging}/backup" "${staging}/installed"
transaction_active=true

rollback() {
  local skill target_skill backup
  set +e
  for skill in "${MANAGED_SKILLS[@]}"; do
    target_skill="${TARGET_DIR}/${skill}"
    backup="${staging}/backup/${skill}"
    if [ -f "${staging}/installed/${skill}" ] && [ -e "$target_skill" ]; then
      rm -rf "$target_skill"
    fi
    if [ -e "$backup" ]; then
      [ ! -e "$target_skill" ] || rm -rf "$target_skill"
      mv "$backup" "$target_skill"
    fi
  done
  set -e
}

cleanup() {
  local status=$?
  trap - EXIT
  if [ "$transaction_active" = true ]; then
    rollback
  fi
  rm -rf "$staging"
  exit "$status"
}
trap cleanup EXIT

for skill in "${MANAGED_SKILLS[@]}"; do
  cp -R "${SOURCE_DIR}/${skill}" "${staging}/new/${skill}"
done

for skill in "${MANAGED_SKILLS[@]}"; do
  target_skill="${TARGET_DIR}/${skill}"
  backup="${staging}/backup/${skill}"
  if [ -d "$target_skill" ]; then
    mv "$target_skill" "$backup"
  fi
  if [ "${ADAPTER_INSTALL_FAIL_ON_SKILL:-}" = "$skill" ]; then
    echo "injected install failure for ${skill}" >&2
    exit 1
  fi
  mv "${staging}/new/${skill}" "$target_skill"
  touch "${staging}/installed/${skill}"
  echo "installed ${skill} -> ${target_skill}"
done

transaction_active=false
echo "SHIFU Codex adapter installed."
