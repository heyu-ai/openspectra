#!/bin/bash
# demo：.openspec.yaml 欄位來源（schema、created_by）、與 new change 比較、衝突、輸出格式
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
prof() { printf '(version 1)\n(allow default)\n(deny network*)\n(deny process-exec (literal "/usr/bin/open"))\n(deny file-write* (subpath "/Users") (subpath "/private/var/folders") (subpath "/private/tmp"))\n(allow file-write* (subpath "%s"))\n' "$1" >| "$W12/demo_$2.sb"; }
sx() { local tag=$1; shift; echo "### ORACLE(sandboxed): $*"; sandbox-exec -f "$W12/demo_$tag.sb" "$O" "$@"; echo "[rc=$?]"; }
mkjail p18; prof "$PWD" p18; commit init
echo "## env identity: GIT_CONFIG_GLOBAL=/dev/null; no repo-local identity"
sx p18 new change via-new
cat openspec/changes/via-new/.openspec.yaml
sx p18 demo
cat openspec/changes/spx-*/.openspec.yaml
echo "## repo-local identity"
git config user.name Local; git config user.email local@x
rm -rf openspec/changes/spx-*
sx p18 demo
cat openspec/changes/spx-*/.openspec.yaml
echo "## custom schema in config + spec_dir other"
mkjail p18b; prof "$PWD" p18b
printf 'spec_dir: docs/sp\n' > .spectra.yaml; mkdir -p docs/sp/changes docs/sp/specs; printf 'schema: custom-x\n' > docs/sp/config.yaml; rm -rf openspec
git config user.name Local; git config user.email local@x
commit init
sx p18b demo
find . -not -path './.git*' -type f | sort
cat docs/sp/changes/spx-*/.openspec.yaml
echo "## --no-color / TTY bytes"
mkjail p18c; prof "$PWD" p18c; commit init
script -q /dev/null sandbox-exec -f "$W12/demo_p18c.sb" "$O" demo | od -c | head -20
sandbox-exec -f "$W12/demo_p18c.sb" "$O" demo --no-color | od -c | head -12
echo "## extra args"
sx p18c demo extra
sx p18c demo --json
