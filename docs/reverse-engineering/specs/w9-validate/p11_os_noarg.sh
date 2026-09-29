#!/bin/bash
# W9 p11：OpenSpectra 與 oracle 的 `validate`（無參數）行為、--changes --specs 同時給、--all 範圍
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
export NO_COLOR=1
cd "$W9/jails/p09-order" || exit 2
echo "== openspectra validate (no args, 4 changes)"; "$S" validate; echo "[rc=$?]"
echo "== openspectra validate --changes --specs"; "$S" validate --changes --specs; echo "[rc=$?]"
echo "== openspec validate --changes --specs"; node "$OSJS" validate --changes --specs --no-interactive; echo "[rc=$?]"
echo "== oracle validate --specs --all --json"; "$O" validate --specs --all --json | jq -c '[.[] | (.change // .spec)]'; echo "[rc=$?]"
echo "== oracle validate --changes --all --json"; "$O" validate --changes --all --json | jq -c '[.[] | (.change // .spec)]'
cd "$W9/jails/p09-specsonly" || exit 2
echo "== openspectra validate (specs only)"; "$S" validate; echo "[rc=$?]"
mkjail p11-one
mkchange solo; good_delta solo caps "S"
echo "== oracle validate (one change)"; "$O" validate; echo "[rc=$?]"
echo "== openspectra validate (one change)"; "$S" validate; echo "[rc=$?]"
