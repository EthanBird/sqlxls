#!/usr/bin/env bash
# 探测一个数据源：列信息、行数、最多 8 行样本。
# 用法:
#   probe.sh -- /path/to/sqlxls locator [WITH 片段]
#   probe.sh locator
#   probe.sh 'sales.xlsx' "format='excel', sheet='Sheet1'"
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

SQLXLS_BIN=""
if [[ "${1:-}" == "--" ]]; then
  shift
  SQLXLS_BIN="${1:?need sqlxls path after --}"
  shift
fi

LOCATOR="${1:-}"
WITH_INNER="${2:-}"
if [[ -z "$LOCATOR" ]]; then
  printf '%s\n' "用法: $0 [-- sqlxls] <locator> [named WITH options]" >&2
  exit 2
fi

if [[ -z "$SQLXLS_BIN" ]]; then
  SQLXLS_BIN="$(bash "$SCRIPT_DIR/ensure-sqlxls.sh")"
fi

esc="${LOCATOR//\'/\'\'}"
if [[ -n "$WITH_INNER" ]]; then
  load="LOAD t FROM '${esc}' WITH (${WITH_INNER});"
else
  load="LOAD t FROM '${esc}';"
fi

printf '== columns ==\n'
"$SQLXLS_BIN" "${load}
PRAGMA table_info(t);"

printf '\n== count ==\n'
"$SQLXLS_BIN" "${load}
SELECT COUNT(*) AS n FROM t;"

printf '\n== sample ==\n'
"$SQLXLS_BIN" "${load}
SELECT * FROM t LIMIT 8;"
