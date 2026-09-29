#!/bin/bash
# p14: validate misc — singular count, JSON trailing bytes, CRLF/BOM schema, extra positional, --json human line absence
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
mkjail p14; mkschema one; mkschema crlf; mkschema bom
python3 - <<'PY'
import re
p='openspec/schemas/one/schema.yaml'; t=open(p).read()
t=t[:t.index('  - id: b')]+'apply:\n  requires: [a]\n'
open(p,'w').write(t)
p='openspec/schemas/crlf/schema.yaml'; t=open(p).read(); open(p,'w',newline='').write(t.replace('\n','\r\n'))
p='openspec/schemas/bom/schema.yaml'; t=open(p).read(); open(p,'w',encoding='utf-8-sig').write(t)
PY
commit init
ora schema validate one
ora schema validate crlf
ora schema validate bom
echo "## JSON tail bytes (valid / invalid)"
"$O" schema validate one --json | tail -c 12 | od -c
"$O" schema validate nosuch --json 2>/dev/null | tail -c 12 | od -c
echo "## extra positional"; ora schema validate one two
echo "## ours on the same"; run3 "$R" schema validate one; run3 "$R" schema validate crlf; run3 "$R" schema validate bom
