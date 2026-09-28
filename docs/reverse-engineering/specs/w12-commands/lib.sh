# source me. Provides: mkjail NAME, $O (oracle), $R (openspectra), commit MSG, mkchange NAME, both
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@x GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@x
export GIT_AUTHOR_DATE="2026-09-01T00:00:00Z" GIT_COMMITTER_DATE="2026-09-01T00:00:00Z"
W12=/Users/howie/.claude/jobs/9eb90dff/tmp/w12
O=/Applications/Spectra.app/Contents/MacOS/spectra
R=/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands/target/release/spectra
# 無網路 sandbox：feedback／demo 一律經由它跑
NONET=$W12/nonet.sb
mkjail() {
  J=$W12/jails/$1; rm -rf "$J"; mkdir -p "$J/openspec/changes/archive" "$J/openspec/specs"
  printf 'spec_dir: openspec\n' > "$J/.spectra.yaml"
  printf 'schema: spec-driven\n' > "$J/openspec/config.yaml"
  cd "$J" || exit 1
  git init -q -b main
}
commit() { git add -A && git -c commit.gpgsign=false commit -q -m "${1:-c}"; }
mkchange() {
  mkdir -p openspec/changes/$1
  printf 'schema: spec-driven\ncreated: 2026-09-01\n' > openspec/changes/$1/.openspec.yaml
}
both() { # args...
  echo "### ORACLE: $*"; "$O" "$@"; echo "[rc=$?]"
  echo "### OPENSPECTRA: $*"; "$R" "$@"; echo "[rc=$?]"
}
oracle() { echo "### ORACLE: $*"; "$O" "$@"; echo "[rc=$?]"; }
oracle_nonet() { echo "### ORACLE(nonet): $*"; sandbox-exec -f "$NONET" "$O" "$@"; echo "[rc=$?]"; }
