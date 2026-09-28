#!/bin/bash
# decisions：封存排序鍵
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
mkjail p07
A=openspec/changes/archive
# 建立順序刻意打亂
dz $A/2026-05-05-mid-b "mid-b"
dz $A/nop-zed "nop-zed"
dz $A/2026-05-05-mid-a "mid-a"
dz $A/2025-01-01-old "old"
dz $A/nop-abc "nop-abc"
dz $A/2026-05-05-mid-c "mid-c"
printf 'schema: spec-driven\ncreated: 2023-01-01\n' > $A/nop-abc/.openspec.yaml
dz $A/2027-01-01-new "new"
dz $A/2026-5-5-short "short"
commit init
oracle decisions
echo "## ls -f order (readdir)"
ls -f $A
echo "## second jail: same names, reverse creation order"
mkjail p07b
A=openspec/changes/archive
dz $A/2026-5-5-short "short"
dz $A/2027-01-01-new "new"
dz $A/2026-05-05-mid-c "mid-c"
dz $A/nop-abc "nop-abc"
printf 'schema: spec-driven\ncreated: 2023-01-01\n' > $A/nop-abc/.openspec.yaml
dz $A/2025-01-01-old "old"
dz $A/2026-05-05-mid-a "mid-a"
dz $A/nop-zed "nop-zed"
dz $A/2026-05-05-mid-b "mid-b"
commit init
oracle decisions
