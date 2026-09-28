#!/bin/bash
# demo：只在 sandbox 內跑——無網路、不可 exec open、寫入只允許 jail 本身。
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
mkprof() { # $1 = jail path
  printf '(version 1)\n(allow default)\n(deny network*)\n(deny process-exec (literal "/usr/bin/open"))\n(deny file-write* (subpath "/Users") (subpath "/private/var/folders") (subpath "/private/tmp"))\n(allow file-write* (subpath "%s"))\n' "$1" >| "$W12/demo_$2.sb"
}
run_demo() { # $1 = tag, rest args
  local tag=$1; shift
  mkprof "$PWD" "$tag"
  echo "### ORACLE(sandboxed): demo $*"; sandbox-exec -f "$W12/demo_$tag.sb" "$O" demo "$@"; echo "[rc=$?]"
}
echo "## control: write outside jail must fail"
mkjail p02ctl; mkprof "$PWD" ctl; sandbox-exec -f "$W12/demo_ctl.sb" /usr/bin/touch "$W12/jails/outside"; echo "[rc=$?]"
sandbox-exec -f "$W12/demo_ctl.sb" /usr/bin/touch inside; echo "[rc=$?] inside ok"

echo "## A: initialized, empty project, committed"
mkjail p02a; commit init
run_demo a
find . -not -path './.git*' | sort
git status --short
echo "## A2: run demo again (collision?)"
run_demo a
git status --short

echo "## B: not initialized (plain dir)"
J=$W12/jails/p02b; rm -rf $J; mkdir -p $J; cd $J
run_demo b
find . | sort

echo "## C: git repo, not initialized"
J=$W12/jails/p02c; rm -rf $J; mkdir -p $J; cd $J; git init -q -b main
run_demo c
find . -not -path './.git*' | sort
