#!/bin/bash
# feedback：只在無網路 sandbox 內跑。先做正向對照證明 sandbox 真的擋網路與 /usr/bin/open。
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
echo "## control: curl inside sandbox (must fail)"
sandbox-exec -f "$NONET" /usr/bin/curl -sS -m 5 -o /dev/null https://example.com; echo "[rc=$?]"
echo "## control: /usr/bin/open inside sandbox (must fail)"
sandbox-exec -f "$NONET" /usr/bin/open -R /; echo "[rc=$?]"
mkjail p01
oracle_nonet feedback
oracle_nonet feedback hello
oracle_nonet feedback hello --body "some details"
oracle_nonet feedback ""
oracle_nonet feedback --body x
oracle_nonet feedback hello --json
oracle_nonet feedback a b
oracle_nonet feedback hello --no-color
echo "## outside a project"
cd /Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails && mkdir -p p01-none && cd p01-none
oracle_nonet feedback hello --body "multi
line"
echo "## files after:"; find /Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails/p01 /Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails/p01-none -newer /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh -not -path '*/.git/*' | sort
echo "## hexdump hello"
cd /Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails/p01; sandbox-exec -f "$NONET" "$O" feedback hello --body "d" | od -c | head
sandbox-exec -f "$NONET" "$O" feedback hello --body "d" 2>&1 >/dev/null | od -c | head
