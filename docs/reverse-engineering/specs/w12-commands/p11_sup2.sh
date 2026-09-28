#!/bin/bash
# decisions：unresolvable 字串形狀、空邊、fence 內的欄位、重複目標
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
base() {
  mkjail "p11-$1"
  mkdir -p openspec/changes/archive/2026-01-15-old
  printf '## Decisions\n\n### Use Redis\n\nold r\n\n### Use Redis\n\ndup\n' > openspec/changes/archive/2026-01-15-old/design.md
  mkchange c
}
one() {
  base "$1"; printf "$2" > openspec/changes/c/design.md
  touch -t 203001010000 openspec/changes/c/design.md
  commit init
  echo "======== case $1"; oracle decisions --json
}
one unres-nospace '## Decisions\n\n### N\n\n**Supersedes**:ghost/X\n'
one unres-spaces '## Decisions\n\n### N\n\n**Supersedes**:   ghost   /   X   \n'
one empty-change '## Decisions\n\n### N\n\n**Supersedes**:  / Use Redis\n'
one empty-head '## Decisions\n\n### N\n\n**Supersedes**: old / \n'
one only-slash '## Decisions\n\n### N\n\n**Supersedes**: /\n'
one in-fence '## Decisions\n\n### N\n\n```\n**Supersedes**: old / Use Redis\n```\n'
one dup-target '## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis\n'
one first-invalid '## Decisions\n\n### N\n\n**Supersedes**: nothing here\n**Supersedes**: old / Use Redis\n'
one first-unres '## Decisions\n\n### N\n\n**Supersedes**: ghost / X\n**Supersedes**: old / Use Redis\n'
one trailing-ws-in-heading-ref '## Decisions\n\n### N\n\n**Supersedes**: old / Use  Redis\n'
one bold-trailing '## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis  \n'
one tab-sep '## Decisions\n\n### N\n\n**Supersedes**:\told\t/\tUse Redis\n'
