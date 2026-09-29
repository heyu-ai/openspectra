#!/bin/bash
# W9 p06：oracle 3.0.0 validate 基本形狀（human／JSON、各 scope、單一 item、未知 item、exit code）
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
mkjail p06
# changes: c-good（有 delta）、b-nodelta（沒有 specs/）、a-badop（specs 下的 spec.md 沒有任何 operation）
mkchange c-good; good_delta c-good cap-one "Thing one"
mkchange b-nodelta
mkchange a-badop; mkdir -p openspec/changes/a-badop/specs/cap-x
printf '# Not a delta\n\nJust notes.\n' > openspec/changes/a-badop/specs/cap-x/spec.md
# specs: cap-one（好）、zz-bad（沒有 ## Requirements）、area/nested（巢狀，好）
good_spec cap-one "Thing zero"
mkdir -p openspec/specs/zz-bad; printf '# zz\n\n## Purpose\n\nSomething long enough to pass purpose length checks here.\n' > openspec/specs/zz-bad/spec.md
good_spec area/nested "Nested thing"
commit init
# 固定 mtime：c-good 最新、a-badop 次之、b-nodelta 最舊（驗證排序是否依 list 的 modified 順序）
find openspec/changes/b-nodelta -type f -exec touch -t 202601010000 {} +
find openspec/changes/a-badop -type f -exec touch -t 202602010000 {} +
find openspec/changes/c-good -type f -exec touch -t 202603010000 {} +
export NO_COLOR=1
oracle list
for args in "validate" "validate --changes" "validate --specs" "validate --all" \
            "validate --json" "validate --changes --json" "validate --specs --json" "validate --all --json" \
            "validate c-good" "validate c-good --json" "validate b-nodelta" "validate a-badop" "validate a-badop --json" \
            "validate cap-one" "validate cap-one --json" "validate zz-bad" "validate zz-bad --json" \
            "validate area/nested" "validate area/nested --json" "validate nope" "validate nope --json" \
            "validate --changes --specs" "validate --changes --specs --json" "validate c-good --all"; do
  echo; echo "================ $args"
  "$O" $args 2> "$W9/jails/p06.err"; rc=$?
  echo "[rc=$rc]"; echo "--- stderr:"; cat "$W9/jails/p06.err"
done
