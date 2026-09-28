#!/bin/bash
# W9 p12：oracle 實際 archive 一個「MODIFIED 漏 scenario」的 change（yibi-mvp 0070），看主 spec 的 scenario 是否消失
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
J=$W9/jails/p12
rm -rf "$J"; cp -R "$W9/corpus/yibi-mvp" "$J" || exit 2
cd "$J" || exit 2
SPEC=docs/openspec/specs/E18-custom-creator/spec.md
cp "$SPEC" "$W9/jails/p12-before.md"
echo "== scenarios named in 0070's omission, before archive:"
grep -n -e 'save-to-content-library' -e 'return-to-script-review' "$SPEC"
echo "== oracle archive 0070-e18-prd-v2-sync --yes"
"$O" archive 0070-e18-prd-v2-sync --yes; echo "[rc=$?]"
echo "== after archive:"
grep -n -e 'save-to-content-library' -e 'return-to-script-review' "$SPEC"; echo "[grep rc=$?]"
diff "$W9/jails/p12-before.md" "$SPEC" | head -60
