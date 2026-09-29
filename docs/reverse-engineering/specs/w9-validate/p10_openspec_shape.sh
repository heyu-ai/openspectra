#!/bin/bash
# W9 p10：OpenSpec 1.13.2 與 OpenSpectra 的 validate --json 頂層形狀、排序、空狀態、錯誤路徑
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
export NO_COLOR=1
cd "$W9/jails/p09-order" || exit 2
echo "######## order (bulk --all)"
node "$OSJS" validate --all --json --no-interactive | jq -c '[.items[] | "\(.type)/\(.id)"]'
"$S" validate --all --json | jq -c '[.items[] | "\(.type)/\(.id)"]'
echo "######## full single-item JSON: change zeta"
node "$OSJS" validate zeta --json --no-interactive; echo "[rc=$?]"
"$S" validate zeta --json; echo "[rc=$?]"
echo "######## bulk --changes JSON (keys only)"
node "$OSJS" validate --changes --json --no-interactive | jq -c 'del(.items) | ., (keys_unsorted)'
"$S" validate --changes --json | jq -c 'del(.items) | ., (keys_unsorted)'
echo "######## item key order"
node "$OSJS" validate --changes --json --no-interactive | jq -c '.items[0] | keys_unsorted'
"$S" validate --changes --json | jq -c '.items[0] | keys_unsorted'
echo "######## ambiguous item 'only'"
node "$OSJS" validate only --json --no-interactive; echo "[rc=$?]"
"$S" validate only --json; echo "[rc=$?]"
node "$OSJS" validate only --no-interactive; echo "[rc=$?]"
"$S" validate only; echo "[rc=$?]"
echo "######## unknown item"
node "$OSJS" validate nope --json --no-interactive; echo "[rc=$?]"
"$S" validate nope --json; echo "[rc=$?]"
node "$OSJS" validate nope --no-interactive; echo "[rc=$?]"
"$S" validate nope; echo "[rc=$?]"
echo "######## spec item"
node "$OSJS" validate a/b --json --no-interactive | jq -c .; echo "[rc=$?]"
"$S" validate a/b --json | jq -c .; echo "[rc=$?]"
echo "######## human bulk"
node "$OSJS" validate --all --no-interactive; echo "[rc=$?]"
"$S" validate --all; echo "[rc=$?]"
echo "######## empty project"
cd "$W9/jails/p09-empty" || exit 2
node "$OSJS" validate --all --json --no-interactive; echo "[rc=$?]"
"$S" validate --all --json; echo "[rc=$?]"
node "$OSJS" validate --changes --json --no-interactive | jq -c .summary
"$S" validate --changes --json | jq -c .summary
node "$OSJS" validate --json --no-interactive; echo "[rc=$?]"
"$S" validate --json; echo "[rc=$?]"
"$S" validate; echo "[rc=$?]"
