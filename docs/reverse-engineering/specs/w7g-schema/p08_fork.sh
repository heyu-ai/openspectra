#!/bin/bash
# p08: schema fork — built-in sources, output, default name, tree contents, git side effects; oracle vs ours
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
G=$W/golden; rm -rf "$G"; mkdir -p "$G"
for who in oracle ours; do
  if [ $who = oracle ]; then B=$O; else B=$R; fi
  echo "################ $who"
  mkjail p08-$who; commit init
  echo "## fork spec-driven f1"; run3 "$B" schema fork spec-driven f1
  echo "## fork no-spec f2 --json"; run3 "$B" schema fork no-spec f2 --json
  echo "## fork spec-driven (default name)"; run3 "$B" schema fork spec-driven
  echo "## fork no-spec (default name) --json"; run3 "$B" schema fork no-spec --json
  echo "## git status after forks (config.yaml untouched?)"; git status --porcelain --untracked-files=all
  echo "## tree"; (cd openspec/schemas && find . -print | sort)
  cp -R openspec/schemas "$G/$who"
done
echo "################ diff oracle vs ours"
diff -r "$G/oracle" "$G/ours"
echo "################ oracle f1/schema.yaml (cat -A-ish via od on first 400 bytes)"
od -c "$G/oracle/f1/schema.yaml" | head -25
echo "################ oracle vs embedded builtin check: validate f1 f2, schemas --json"
cd "$W/jails/p08-oracle"; ora schema validate f1; ora schema validate f2; ora schema validate spec-driven-custom
"$O" schemas --json | python3 -c "import json,sys; [print(s) for s in json.load(sys.stdin)]"
echo "## which f1"; "$O" schema which f1
