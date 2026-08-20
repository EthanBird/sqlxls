#!/usr/bin/env bash
# 在 stdout 打印可用的 sqlxls 可执行文件路径。日志走 stderr。
set -euo pipefail

RELEASE_TAG="${SQLXLS_RELEASE_TAG:-v0.3.0}"
REPO="${SQLXLS_REPO:-EthanBird/sqlxls}"
CACHE_DIR="${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/sqlxls/bin"

log() { printf '%s\n' "$*" >&2; }

is_exec() {
  [[ -n "${1:-}" && -f "$1" && -x "$1" ]]
}

# 1) 显式路径
if is_exec "${SQLXLS:-}"; then
  printf '%s\n' "$SQLXLS"
  exit 0
fi

# 2) PATH
if command -v sqlxls >/dev/null 2>&1; then
  command -v sqlxls
  exit 0
fi

# 3) 从技能包或 cwd 向上找仓库里的 release 二进制
find_in_repo() {
  local dir="$1"
  local i
  for i in 1 2 3 4 5 6 7 8; do
    if [[ -x "$dir/target/release/sqlxls" ]]; then
      printf '%s\n' "$dir/target/release/sqlxls"
      return 0
    fi
    [[ "$dir" == "/" ]] && break
    dir="$(cd "$dir/.." && pwd)"
  done
  return 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if found="$(find_in_repo "$SCRIPT_DIR")"; then
  printf '%s\n' "$found"
  exit 0
fi
if found="$(find_in_repo "$(pwd)")"; then
  printf '%s\n' "$found"
  exit 0
fi

# 4) 缓存里已有
mkdir -p "$CACHE_DIR"
CACHED="$CACHE_DIR/sqlxls"
if is_exec "$CACHED"; then
  printf '%s\n' "$CACHED"
  exit 0
fi

# 5) 下载 GitHub Release
uname_s="$(uname -s)"
uname_m="$(uname -m)"
asset=""
case "$uname_s/$uname_m" in
  Linux/x86_64|Linux/amd64) asset="sqlxls-x86_64-unknown-linux-gnu" ;;
  Darwin/arm64) asset="sqlxls-aarch64-apple-darwin" ;;
  Darwin/x86_64) asset="sqlxls-x86_64-apple-darwin" ;;
  *)
    log "无法自动安装：不支持的平台 $uname_s/$uname_m"
    log "请从 https://github.com/${REPO}/releases 手动下载 ${RELEASE_TAG}"
    exit 1
    ;;
esac

url="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/${asset}"
log "正在下载 ${url}"
tmp="$(mktemp)"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL -o "$tmp" "$url"
elif command -v wget >/dev/null 2>&1; then
  wget -q -O "$tmp" "$url"
else
  log "需要 curl 或 wget 才能下载 sqlxls"
  rm -f "$tmp"
  exit 1
fi
chmod +x "$tmp"
mv "$tmp" "$CACHED"
printf '%s\n' "$CACHED"
