#!/bin/bash
# 未初始化目錄：decisions／demo／show 新旗標的錯誤。
# 注意：job tmp 的上層 /Users/howie/.claude/jobs/9eb90dff/tmp/ 有別的 agent 放的 openspec/（npm 套件）與 .git，
# oracle 會向上找到它當成專案根（首輪 p15 因此把 demo change 寫到 tmp/openspec/changes/，已清除）。
# 所以這裡改在 $TMPDIR 下建唯一目錄，祖先沒有任何 openspec/.spectra.yaml；sandbox 只允許寫該目錄。
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
BASE=$(mktemp -d "${TMPDIR:-/private/var/tmp}/w12-uninit.XXXXXX")
BASE=$(cd "$BASE" && pwd -P)
echo "BASE=$BASE"
d=$BASE; while [ "$d" != "/" ]; do d=$(dirname "$d"); if [ -e "$d/openspec" ] || [ -e "$d/.spectra.yaml" ]; then echo "[FAIL] marker in ancestor $d"; exit 1; fi; done
printf '(version 1)\n(allow default)\n(deny network*)\n(deny process-exec (literal "/usr/bin/open"))\n(deny file-write* (subpath "/Users") (subpath "/private/var/folders") (subpath "/private/tmp"))\n(allow file-write* (subpath "%s"))\n' "$BASE" >| $W12/uninit.sb
sx() { echo "### ORACLE(sandboxed): $*"; sandbox-exec -f $W12/uninit.sb "$O" "$@"; echo "[rc=$?]"; }
mkdir -p $BASE/plain; cd $BASE/plain
sx decisions
sx decisions --json
sx show --deltas-only
sx show x -r
sx demo
find . | sort
mkdir -p $BASE/gitrepo; cd $BASE/gitrepo; git init -q -b main
sx decisions
sx demo
find . -not -path './.git/*' | sort
rm -rf "$BASE"
