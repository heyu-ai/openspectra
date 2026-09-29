#!/bin/bash
# p06: what the oracle does downstream with schemas that `validate` accepts but look broken.
# Reuses the p04 jails (read-only commands only).
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
J=$W/jails
q() { python3 -c "import json,sys; d=json.load(sys.stdin); print({k: d.get(k) for k in sys.argv[1:]})" "$@"; }

for c in tpl-missing tpl-empty tpl-dir-missing tpl-traversal tpl-traversal-missing tpl-absolute tpl-subdir; do
  echo "##### $c: instructions b --json -> template field"
  cd "$J/p04-$c"; "$O" instructions b --change c1 --json >| "$W/.o" 2>&1; echo "rc=$?"; q template outputPath < "$W/.o" 2>/dev/null || head -3 "$W/.o"
  echo "## human instructions b (template section)"; "$O" instructions b --change c1 2>&1 | grep -n -A3 -i "template" | head -8
  echo "## templates b"; "$O" templates --schema m 2>&1 | head -5
done

for c in gen-absolute gen-traversal; do
  echo "##### $c"
  cd "$J/p04-$c"; "$O" instructions b --change c1 --json >| "$W/.o" 2>&1; echo "rc=$?"; q outputPath < "$W/.o" 2>/dev/null || head -3 "$W/.o"
  "$O" status --change c1 2>&1 | head -8
done

echo "##### no-apply: instructions apply --json (top keys + instruction/tracks)"
cd "$J/p04-no-apply"; "$O" instructions apply --change c1 --json >| "$W/.o" 2>&1; echo "rc=$?"
python3 -c "import json; d=json.load(open('$W/.o')); print(list(d)); print({k: d.get(k) for k in ('state','instruction','contextFiles','progress')})"
echo "##### art-no-instruction: instructions a --json instruction field"
cd "$J/p04-art-no-instruction"; "$O" instructions a --change c1 --json >| "$W/.o" 2>&1; echo "rc=$?"; q instruction < "$W/.o"
echo "##### no-description / description-null: schemas --json entry for m"
for c in no-description description-null name-mismatch name-empty; do
  cd "$J/p04-$c"; echo "## $c"; "$O" schemas --json | python3 -c "import json,sys; print([s for s in json.load(sys.stdin) if s['name'] in ('m','other','')])"
  "$O" status --change c1 --json | python3 -c "import json,sys; d=json.load(sys.stdin); print('status schemaName=', d.get('schemaName'))"
  "$O" schema which m 2>&1
done
