#!/bin/bash
# decisions：隱藏目錄、archive 巢狀、changes/archive 本身有 design.md
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
mkjail p20
C=openspec/changes
dz $C/.hidden "hidden-active"
dz $C/archive/.hidden-arch "hidden-archived"
dz $C/archive "archive-dir-itself"
dz $C/archive/2026-01-01-x/nested "nested-in-archive"
dz $C/Upper_Case "upper"
dz $C/archive/2026-01-01-x "arch-x"
commit init
oracle decisions --json
