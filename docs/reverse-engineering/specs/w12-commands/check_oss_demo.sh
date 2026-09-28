#!/bin/bash
# OpenSpectra demo：與 oracle 擷取內容逐位元組比對；未初始化目錄的錯誤。
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
mkjail oss-demo; git config user.name T; git config user.email t@x; commit init
fails=0
for i in $(seq 1 60); do
  out=$("$R" demo) || { echo "[FAIL] rc"; exit 1; }
  name=$(printf '%s\n' "$out" | sed -n 's/^✓ Created demo change: //p')
  theme=$(printf '%s\n' "$out" | sed -n 's/^  Theme: //p')
  d=openspec/changes/$name
  for f in proposal.md design.md tasks.md specs/$theme/spec.md; do
    cmp -s "$d/$f" "$W12/harvest/$theme/$f" || { echo "[FAIL] $name $f"; fails=1; }
  done
  n=$(find "$d" -type f | wc -l | tr -d ' ')
  [ "$n" = 5 ] || { echo "[FAIL] $name has $n files"; fails=1; }
  printf '%s\n' "$theme"
done | sort | uniq -c
printf '%s\n' "$("$R" demo)"
cat openspec/changes/spx-*/.openspec.yaml | sort | uniq -c
ls -A
echo "fails=$fails"
BASE=$(mktemp -d "${TMPDIR:-/private/var/tmp}/w12-oss-uninit.XXXXXX"); cd "$BASE"
for a in "demo" "decisions" "show x -r" "show --deltas-only"; do echo "### OSS: $a"; "$R" $a; echo "[rc=$?]"; done
echo "### OSS: feedback outside project"; "$R" feedback hi; echo "[rc=$?]"
cd /; rm -rf "$BASE"
