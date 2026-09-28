#!/bin/bash
# p11: where did `schema fork no-spec g1` write when run outside a project (p10 G)? Search likely roots.
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
for d in "$HOME/Library/Application Support" "$HOME/.config" "$HOME/.local/share" "$HOME/.spectra" "$W/jails"; do
  [ -d "$d" ] && find "$d" -maxdepth 5 -type d \( -name g1 -o -name 'g1*' \) -print 2>/dev/null
done
echo "## oracle schema which g1 (from the p10g-oracle jail)"
cd "$W/jails/p10g-oracle"; "$O" schema which g1 --json
echo "## oracle schema which --all --json"
"$O" schema which no-spec --all --json
