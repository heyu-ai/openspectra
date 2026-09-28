#!/bin/bash
# p07: colours on a terminal (script(1) PTY) for validate success / failure, and fork; --no-color / NO_COLOR
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
unset NO_COLOR
cd "$W/jails/p04-valid" || exit 1
for args in "schema validate m" "schema validate m --no-color" "schema validate nosuch" "schema validate nosuch --no-color"; do
  echo "=== TTY oracle: $args"
  script -q /dev/null "$O" $args < /dev/null | od -c | head -12
done
echo "=== TTY oracle NO_COLOR=1: schema validate nosuch"
NO_COLOR=1 script -q /dev/null "$O" schema validate nosuch < /dev/null | od -c | head -12
echo "=== TTY oracle: fork in fresh jail"
mkjail p07fork; commit init
script -q /dev/null "$O" schema fork no-spec zz < /dev/null | od -c | head -12
script -q /dev/null "$O" schema fork no-spec zz < /dev/null | od -c | head -12
