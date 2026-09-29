#!/bin/bash
# W9 p01：oracle validate 的 human／JSON 形狀（parity sandbox 的 corpus 副本，唯讀）
O=/Applications/Spectra.app/Contents/MacOS/spectra
B=/var/folders/27/lz5ljhb96m5_l7n92_xhn9rm0000gn/T/parity-probe-ye0mtycd
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
export NO_COLOR=1
for d in yibi-mvp nextrek-cli yibi-stack; do
  cd "$B/$d" || exit 2
  for args in "validate --changes" "validate --specs" "validate --all" "validate --changes --json" "validate --specs --json" "validate --all --json"; do
    tag=$(echo "$d $args" | tr ' ' '_')
    "$O" $args >| "$W/p01-$tag.out" 2>| "$W/p01-$tag.err"; echo "rc=$?" >> "$W/p01-$tag.err"
  done
done
cd "$B/yibi-mvp" || exit 2
echo "== human --changes (head)"; head -30 "$W/p01-yibi-mvp_validate_--changes.out" | cat -e | head -30
echo "== stderr"; cat "$W/p01-yibi-mvp_validate_--changes.err"
echo "== json --changes (first 2 items)"; jq '.[0:2]' "$W/p01-yibi-mvp_validate_--changes_--json.out"
echo "== json item shapes"; jq -c '[.[] | keys] | unique' "$W/p01-yibi-mvp_validate_--all_--json.out"
echo "== any warnings/errors"; jq -c '[.[] | select((.errors|length)>0 or (.warnings|length)>0)] | .[0:3]' "$W/p01-yibi-mvp_validate_--all_--json.out"
