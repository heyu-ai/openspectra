#!/bin/bash
# demo 在未初始化目錄：HOME 指向 jail，sandbox 只允許寫 jail。
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
prof() {
  printf '(version 1)\n(allow default)\n(deny network*)\n(deny process-exec (literal "/usr/bin/open"))\n(deny file-write* (subpath "/Users") (subpath "/private/var/folders") (subpath "/private/tmp"))\n(allow file-write* (subpath "%s"))\n' "$1" >| "$W12/demo_$2.sb"
}
for tag in b c; do
  J=$W12/jails/p03$tag; rm -rf $J; mkdir -p $J/home $J/proj; cd $J/proj
  [ $tag = c ] && git init -q -b main
  prof "$J" $tag
  echo "## $tag (c = git repo)"
  echo "### ORACLE(sandboxed, HOME=jail/home): demo"
  mkdir -p $J/tmp; TMPDIR=$J/tmp/ HOME=$J/home sandbox-exec -f "$W12/demo_$tag.sb" "$O" demo; echo "[rc=$?]"
  echo "### ORACLE(sandboxed, HOME=jail/home): list"
  mkdir -p $J/tmp; TMPDIR=$J/tmp/ HOME=$J/home sandbox-exec -f "$W12/demo_$tag.sb" "$O" list; echo "[rc=$?]"
  cd $J; find . -not -path '*/.git/*' | sort
done
