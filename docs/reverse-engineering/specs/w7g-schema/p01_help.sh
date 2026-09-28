#!/bin/bash
# p01: help texts for schema, schema validate, schema fork; version
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
"$O" --version
for a in "schema --help" "schema validate --help" "schema fork --help"; do echo "=== oracle $a"; "$O" $a; echo "[rc=$?]"; done
for a in "schema validate --help" "schema fork --help"; do echo "=== ours $a"; "$R" $a; echo "[rc=$?]"; done
