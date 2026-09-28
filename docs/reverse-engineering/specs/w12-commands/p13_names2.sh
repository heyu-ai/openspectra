#!/bin/bash
# decisions：前綴剝除條件的判別、日期樣式、Unicode 大小寫
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
mkjail p13
A=openspec/changes/archive
for d in 2026-abcdefgh abcd-efghijkl 12345-abcdefg 202-5-abcdefgh 2026-05-abcdefgh abcd-ef-gh-rest 2026-0a-05-xyz 9999-99-99-nine 2026_05_05-und 2026-05-05-2026-06-06-double x026-05-05-x 2026--5-05-zz; do
  dz "$A/$d" "dir=$d"
done
commit init
oracle decisions --json
mkjail p13b
mkchange u
printf '## Decisions\n\n### ÄBC Straße\n\nİstanbul ǅ\n' > openspec/changes/u/design.md
commit init
for k in äbc ÄBC strasse STRASSE straße i̇stanbul istanbul ǆ ǅ Ǆ; do oracle decisions "$k"; done
