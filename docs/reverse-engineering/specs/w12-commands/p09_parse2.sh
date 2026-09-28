#!/bin/bash
# decisions：fence 與 section 結束的邊界
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
one() {
  mkjail "p09-$1"; mkchange c; printf "$2" > openspec/changes/c/design.md; commit init
  echo "======== case $1"; oracle decisions --json
}
one tilde '## Decisions\n\n### A\n\n~~~\n### InTilde\n~~~\n\nx\n'
one fence-indent '## Decisions\n\n### A\n\n  ```\n### InIndented\n  ```\n\nx\n'
one fence-lang '## Decisions\n\n### A\n\n```rust\n### InLang\n```\n\nx\n'
one fence-unclosed '## Decisions\n\n### A\n\n```\n### InUnclosed\n\n## After\n'
one fence-4 '## Decisions\n\n### A\n\n````\n```\n### InFour\n````\n\nx\n'
one fence-close-lang '## Decisions\n\n### A\n\n```\n### In1\n```rust\n### In2\n```\n\nx\n'
one sect-in-fence '```\n## Decisions\n```\n\n### A\n\nr\n'
one end-bare '## Decisions\n\n### A\n\nr\n##\nafter\n'
one end-tab '## Decisions\n\n### A\n\nr\n##\tTab\nafter\n'
one end-emptyname '## Decisions\n\n### A\n\nr\n## \nafter\n'
one end-hashes '## Decisions\n\n### A\n\nr\n##Nospace\nafter\n'
one sect-tab '## Decisions\t\n\n### A\n\nr\n'
one sect-fence-inside-then-real '## Decisions\n\n```\n### Fenced\n```\n\n### Real\n\nr\n'
