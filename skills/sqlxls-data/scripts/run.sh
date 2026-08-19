#!/usr/bin/env bash
# 以 --strict 执行 .sql 脚本。
# 用法: run.sh [--explain] [-o out.xlsx] script.sql
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SQLXLS_BIN="$(bash "$SCRIPT_DIR/ensure-sqlxls.sh")"

explain=0
out=""
file=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --explain) explain=1; shift ;;
    -o|--output)
      out="${2:?need output path}"
      shift 2
      ;;
    -h|--help)
      printf '%s\n' "用法: $0 [--explain] [-o out.xlsx] script.sql" >&2
      exit 0
      ;;
    --) shift; break ;;
    -*)
      printf '未知参数: %s\n' "$1" >&2
      exit 2
      ;;
    *)
      file="$1"
      shift
      ;;
  esac
done

if [[ -z "$file" ]]; then
  printf '%s\n' "用法: $0 [--explain] [-o out.xlsx] script.sql" >&2
  exit 2
fi
if [[ ! -f "$file" ]]; then
  printf '找不到 SQL 文件: %s\n' "$file" >&2
  exit 1
fi

args=("$SQLXLS_BIN" "$file" --strict)
if [[ "$explain" -eq 1 ]]; then
  args+=(--explain)
fi
if [[ -n "$out" ]]; then
  args+=(-o "$out")
fi
exec "${args[@]}"
