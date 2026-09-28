#!/bin/bash
# decisions：design.md 解析——section 判定、heading 形狀、fence、CRLF、rationale 範圍
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
one() { # name, content(printf-format)
  mkjail "p08-$1"; mkchange c; printf "$2" > openspec/changes/c/design.md; commit init
  echo "======== case $1"; oracle decisions --json
}
one sect-trailing '## Decisions  \n\n### A\n\nr\n'
one sect-key '## Key Decisions\n\n### A\n\nr\n'
one sect-lower '## decisions\n\n### A\n\nr\n'
one sect-h1 '# Decisions\n\n### A\n\nr\n'
one sect-h3 '### Decisions\n\n### A\n\nr\n'
one sect-suffix '## Decisions and Rationale\n\n### A\n\nr\n'
one sect-colon '## Decisions:\n\n### A\n\nr\n'
one sect-none '## Context\n\n### A\n\nr\n'
one sect-nospace '##Decisions\n\n### A\n\nr\n'
one sect-indent ' ## Decisions\n\n### A\n\nr\n'
one sect-two '## Decisions\n\n### A\n\nr\n\n## Other\n\n### X\n\n## Decisions\n\n### B\n\nr2\n'
one end-h1 '## Decisions\n\n### A\n\nr\n\n# Top\n\nafter top\n'
one end-h4 '## Decisions\n\n### A\n\nr\n#### deep\nd\n##### deeper\n'
one hd-nospace '## Decisions\n\n###A\n\nr\n\n### B\n\nr2\n'
one hd-empty '## Decisions\n\n### \n\nr\n\n###\n\nr3\n\n### B\n\nr2\n'
one hd-trailing '## Decisions\n\n###   Spaced   \n\nr\n'
one hd-closing '## Decisions\n\n### Foo ###\n\nr\n'
one hd-indent '## Decisions\n\n  ### Indented\n\nr\n\n### B\n\nr2\n'
one hd-tab '## Decisions\n\n###\tTab\n\nr\n'
one fence '## Decisions\n\n### A\n\n```\n### NotHeading\n## NotEnd\n```\n\nafter fence\n'
one crlf '## Decisions\r\n\r\n### A\r\n\r\nline1\r\nline2\r\n\r\n### B\r\n\r\nr2\r\n'
one ws-rationale '## Decisions\n\n### A\n\n   \n  indented first  \n\n  trailing  \n\n\n'
one html-comment '## Decisions\n\n<!-- comment -->\n\n### A\n\n<!-- inside -->\nr\n'
one dup '## Decisions\n\n### Same\n\none\n\n### Same\n\ntwo\n'
one bom '\xef\xbb\xbf## Decisions\n\n### A\n\nr\n'
one first-line '### A\n\nr\n'
