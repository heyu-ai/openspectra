#!/bin/bash
# decisions：**Supersedes** 欄位的格式與解析
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
base() {
  mkjail "p10-$1"
  mkdir -p openspec/changes/archive/2026-01-15-old
  printf '## Decisions\n\n### Use Redis\n\nold r\n\n### Other Old\n\no\n' > openspec/changes/archive/2026-01-15-old/design.md
  mkchange act
  printf '## Decisions\n\n### Active Target\n\nat\n' > openspec/changes/act/design.md
  mkchange c
}
one() {
  base "$1"; printf "$2" > openspec/changes/c/design.md
  touch -t 203001010000 openspec/changes/c/design.md
  commit init
  echo "======== case $1"; oracle decisions --json
}
one ok '## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis\n\nr\n'
one no-blank '## Decisions\n\n### N\n**Supersedes**: old / Use Redis\nr\n'
one later-para '## Decisions\n\n### N\n\nfirst para\n\n**Supersedes**: old / Use Redis\n'
one dated '## Decisions\n\n### N\n\n**Supersedes**: 2026-01-15-old / Use Redis\n'
one to-active '## Decisions\n\n### N\n\n**Supersedes**: act / Active Target\n'
one case-head '## Decisions\n\n### N\n\n**Supersedes**: old / use redis\n'
one case-change '## Decisions\n\n### N\n\n**Supersedes**: OLD / Use Redis\n'
one nospace '## Decisions\n\n### N\n\n**Supersedes**:old/Use Redis\n'
one extra-space '## Decisions\n\n### N\n\n**Supersedes**:   old   /   Use Redis   \n'
one colon-inside '## Decisions\n\n### N\n\n**Supersedes:** old / Use Redis\n'
one plain '## Decisions\n\n### N\n\nSupersedes: old / Use Redis\n'
one lower '## Decisions\n\n### N\n\n**supersedes**: old / Use Redis\n'
one noslash '## Decisions\n\n### N\n\n**Supersedes**: old Use Redis\n'
one empty '## Decisions\n\n### N\n\n**Supersedes**:\n'
one two-slash '## Decisions\n\n### N\n\n**Supersedes**: old / Use / Redis\n'
one self '## Decisions\n\n### N\n\n**Supersedes**: c / N\n'
one same-change '## Decisions\n\n### First\n\nf\n\n### N\n\n**Supersedes**: c / First\n'
one twice '## Decisions\n\n### N1\n\n**Supersedes**: old / Use Redis\n\n### N2\n\n**Supersedes**: old / Use Redis\n'
one two-fields '## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis\n**Supersedes**: old / Other Old\n'
one list-item '## Decisions\n\n### N\n\n- **Supersedes**: old / Use Redis\n'
one indented '## Decisions\n\n### N\n\n  **Supersedes**: old / Use Redis\n'
one trailing-text '## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis (because x)\n'
one ghost-change '## Decisions\n\n### N\n\n**Supersedes**: ghost / Use Redis\n'
one ghost-head '## Decisions\n\n### N\n\n**Supersedes**: old / Nope\n'
one heading-slash '## Decisions\n\n### A / B\n\nx\n\n### N\n\n**Supersedes**: c / A / B\n'
