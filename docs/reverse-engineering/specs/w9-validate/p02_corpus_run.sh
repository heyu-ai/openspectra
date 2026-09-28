#!/bin/bash
# W9 p02：把 corpus 複製到 w9re/corpus/，加 openspec symlink，三個工具各跑 validate --json
# 方法：OpenSpec 1.13.2 固定從 cwd 讀 openspec/{changes,specs}（item-discovery.js:29、
# workflow/shared.js:77），所以 spec_dir=docs/openspec 的專案在根目錄加 `openspec -> docs/openspec`。
set -u
O=/Applications/Spectra.app/Contents/MacOS/spectra
S=/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w8-locale/target/release/spectra
OSJS=/Users/howie/.npm/_npx/0aaef5be8686a8bb/node_modules/@fission-ai/openspec/bin/openspec.js
B=/var/folders/27/lz5ljhb96m5_l7n92_xhn9rm0000gn/T/parity-probe-ye0mtycd
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
C=$W/corpus
R=$W/p02
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 NO_COLOR=1 OPENSPEC_TELEMETRY=0 DO_NOT_TRACK=1 CI=1
mkdir -p "$C" "$R"
for d in yibi-mvp nextrek-cli yibi-stack; do
  if [ ! -d "$C/$d" ]; then
    cp -R "$B/$d" "$C/$d" || exit 2
    if [ -d "$C/$d/docs/openspec" ] && [ ! -e "$C/$d/openspec" ]; then
      ln -s docs/openspec "$C/$d/openspec"
    fi
  fi
done
for d in yibi-mvp nextrek-cli yibi-stack; do
  cd "$C/$d" || exit 2
  for scope in changes specs; do
    "$O" validate --$scope --json >| "$R/$d.$scope.oracle.json" 2>| "$R/$d.$scope.oracle.err"; echo "rc=$?" >> "$R/$d.$scope.oracle.err"
    "$S" validate --$scope --json >| "$R/$d.$scope.os.json" 2>| "$R/$d.$scope.os.err"; echo "rc=$?" >> "$R/$d.$scope.os.err"
    node "$OSJS" validate --$scope --json --no-interactive >| "$R/$d.$scope.openspec.json" 2>| "$R/$d.$scope.openspec.err"; echo "rc=$?" >> "$R/$d.$scope.openspec.err"
    node "$OSJS" validate --$scope --json --strict --no-interactive >| "$R/$d.$scope.openspec-strict.json" 2>| "$R/$d.$scope.openspec-strict.err"; echo "rc=$?" >> "$R/$d.$scope.openspec-strict.err"
  done
done
cd "$R" || exit 2
tail -n 3 ./*.err
