#!/bin/bash
# decisions：封存目錄名稱→change 名稱與日期；keyword 過濾與 supersession 的先後
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
mkjail p12
A=openspec/changes/archive
for d in 2026-5-5-short 1x-abcdefghijk 2026abcdefghijk 9-short-name-x 2026-05-05 2026-05-05- 2026-05-05x 2026-05-05-x 2026-5 20260505-abcdef ab-cd-ef-gh-ij abcdefghijklmnop 2026-05-05_under 0000-00-00-zero; do
  dz "$A/$d" "dir=$d"
done
commit init
oracle decisions --json
mkjail p12b
mkdir -p openspec/changes/archive/2026-01-15-old
printf '## Decisions\n\n### Use Redis\n\nfast cache\n' > openspec/changes/archive/2026-01-15-old/design.md
mkchange newer
printf '## Decisions\n\n### Replace\n\n**Supersedes**: old / Use Redis\n\nin-process\n\n### Bad\n\n**Supersedes**: ghost / G\n' > openspec/changes/newer/design.md
commit init
oracle decisions fast
oracle decisions fast --json
oracle decisions newer
oracle decisions in-process
oracle decisions ghost
oracle decisions ""
oracle decisions "2026-09-01"
oracle decisions "REPLACE"
oracle decisions "  replace  "
echo "## one decision (singular?)"
oracle decisions Bad
