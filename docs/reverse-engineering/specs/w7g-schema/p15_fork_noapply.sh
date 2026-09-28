#!/bin/bash
# p15: fork serialization of a project schema without `apply`, and with templates/ dir absent; empty-name target with no schemas dir
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
mkjail p15a; mkschema m
python3 - <<'PY'
p='openspec/schemas/m/schema.yaml'; t=open(p).read(); open(p,'w').write(t[:t.index('apply:')])
PY
rm -rf openspec/schemas/m/templates; commit init
ora schema fork m m2; find openspec/schemas/m2 -print | sort; cat openspec/schemas/m2/schema.yaml
echo "##### empty target when openspec/schemas does not exist yet"
mkjail p15b; commit init
ora schema fork no-spec ""; find openspec -print | sort
