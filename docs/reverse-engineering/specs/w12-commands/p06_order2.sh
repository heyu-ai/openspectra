#!/bin/bash
# decisions：mtime 是否決定排序？封存與進行中是否交錯？
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
settime() { find "$1" -type f -exec touch -t "$2" {} +; }
mkjail p06
C=openspec/changes
mkchange a1; dz $C/a1 "H-a1"
mkchange a2; dz $C/a2 "H-a2"
mkchange a3; dz $C/a3 "H-a3"
dz $C/archive/2026-05-05-r1 "H-r1"
dz $C/archive/2026-05-06-r2 "H-r2"
dz $C/archive/2026-05-04-r3 "H-r3"
commit init
settime $C/a1 202601010000
settime $C/a2 202603010000
settime $C/a3 202602010000
settime $C/archive/2026-05-05-r1 202701010000
settime $C/archive/2026-05-06-r2 202501010000
settime $C/archive/2026-05-04-r3 202601150000
oracle decisions
echo "## now a non-design file newest in a1"
touch -t 202801010000 $C/a1/tasks.md
oracle decisions
echo "## archives: swap mtimes"
settime $C/archive/2026-05-05-r1 202401010000
settime $C/archive/2026-05-06-r2 202901010000
oracle decisions
echo "## list --sort modified for reference (archives not listed)"
oracle list
