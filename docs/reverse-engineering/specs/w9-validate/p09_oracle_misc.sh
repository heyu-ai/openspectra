#!/bin/bash
# W9 p09：oracle 的空狀態、TTY 顏色、--no-color、排序（changes 對 list、specs 字串序）、名稱衝突
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
export NO_COLOR=1
echo "######## empty project"
mkjail p09-empty
for a in "validate" "validate --json" "validate --changes" "validate --changes --json" "validate --specs" "validate --specs --json" "validate --all" "validate --all --json"; do
  echo "== $a"; "$O" $a; echo "[rc=$?]"
done
echo "######## specs only, no changes"
mkjail p09-specsonly
good_spec only "Only"
for a in "validate" "validate --json" "validate --all --json" "validate --specs"; do
  echo "== $a"; "$O" $a; echo "[rc=$?]"
done
echo "######## ordering + collision"
mkjail p09-order
for n in a-b a.b B-upper aa; do good_spec "$n" "R $n"; done
good_spec a/b "R ab"
good_spec parentless/child "R pc"
mkdir -p openspec/specs/nospecmd; printf 'x\n' > openspec/specs/nospecmd/readme.md
mkchange zeta; good_delta zeta capz "Z"
mkchange Upper_Case; good_delta Upper_Case capu "U"
mkchange only; good_delta only capo "O"   # 名稱與下面的 spec 相同
good_spec only "Only spec"
mkchange alpha; good_delta alpha capa "A"
commit init
find openspec/changes/alpha -type f -exec touch -t 202601010000 {} +
find openspec/changes/zeta -type f -exec touch -t 202603010000 {} +
find openspec/changes/only -type f -exec touch -t 202602010000 {} +
find openspec/changes/Upper_Case -type f -exec touch -t 202602150000 {} +
"$O" list --json | jq -c '[.changes[].name]'
"$O" validate --json | jq -c '[.[].change]'
"$O" list --specs --json | jq -c '[.specs[].id]'
"$O" validate --specs --json | jq -c '[.[].spec]'
echo "== validate only (collision)"; "$O" validate only --json; echo "[rc=$?]"
echo "== validate Upper_Case"; "$O" validate Upper_Case; echo "[rc=$?]"
echo "######## TTY colours (script -q)"
unset NO_COLOR
cd "$W9/jails/p08" || exit 2
script -q /dev/null "$O" validate d23-mixed | cat -v
script -q /dev/null "$O" validate d19-skipspecs | cat -v
script -q /dev/null "$O" validate d01-good | cat -v
echo "== --no-color on TTY"
script -q /dev/null "$O" validate d23-mixed --no-color | cat -v
echo "== NO_COLOR=1 on TTY"
NO_COLOR=1 script -q /dev/null "$O" validate d23-mixed | cat -v
echo "== TTY json"
script -q /dev/null "$O" validate d19-skipspecs --json | cat -v
echo "== piped (no tty) without NO_COLOR"
"$O" validate d23-mixed 2>&1 | cat -v
echo "######## not initialized"
mkdir -p "$W9/jails/p09-noinit"; cd "$W9/jails/p09-noinit" || exit 2
"$O" validate; echo "[rc=$?]"
"$O" validate --json; echo "[rc=$?]"
