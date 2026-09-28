#!/bin/bash
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
grep -n -B1 -A3 -e '✗' -e 'error' "$W/p01-yibi-mvp_validate_--specs.out" | head -60
echo ===
cat "$W/p01-nextrek-cli_validate_--all.out"
cat "$W/p01-nextrek-cli_validate_--specs.out" "$W/p01-nextrek-cli_validate_--specs.err"
cat "$W/p01-nextrek-cli_validate_--specs_--json.out"
echo
cat "$W/p01-yibi-stack_validate_--all.out" "$W/p01-yibi-stack_validate_--all.err"
echo === specs json yibi-mvp invalid
jq -c '.[] | select(.valid==false)' "$W/p01-yibi-mvp_validate_--specs_--json.out"
jq -c '.[0]' "$W/p01-yibi-mvp_validate_--specs_--json.out"
jq -c '[.[] | keys] | unique' "$W/p01-yibi-mvp_validate_--specs_--json.out"
