# source me. Provides: mkjail NAME, $O (oracle), $S (openspectra), $OSJS (OpenSpec 1.13.2),
# commit MSG, mkchange NAME, good_delta CAP [REQ], good_spec CAP [REQ], run3 ARGS...
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@x GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@x
export GIT_AUTHOR_DATE="2026-09-01T00:00:00Z" GIT_COMMITTER_DATE="2026-09-01T00:00:00Z"
export OPENSPEC_TELEMETRY=0 DO_NOT_TRACK=1 CI=1
unset FORCE_COLOR
W9=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
O=/Applications/Spectra.app/Contents/MacOS/spectra
S=/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w8-locale/target/release/spectra
OSJS=/Users/howie/.npm/_npx/0aaef5be8686a8bb/node_modules/@fission-ai/openspec/bin/openspec.js
mkjail() {
  J=$W9/jails/$1; rm -rf "$J"; mkdir -p "$J/openspec/changes/archive" "$J/openspec/specs"
  printf 'spec_dir: openspec\n' > "$J/.spectra.yaml"
  printf 'schema: spec-driven\n' > "$J/openspec/config.yaml"
  cd "$J" || exit 1
  git init -q -b main
}
commit() { git add -A && git -c commit.gpgsign=false commit -q -m "${1:-c}"; }
mkchange() {
  mkdir -p "openspec/changes/$1"
  printf 'schema: spec-driven\ncreated: 2026-09-01\n' > "openspec/changes/$1/.openspec.yaml"
  printf '## Why\n\nBecause.\n\n## What Changes\n\n- x\n' > "openspec/changes/$1/proposal.md"
}
# good_delta CHANGE CAP REQ
good_delta() {
  mkdir -p "openspec/changes/$1/specs/$2"
  printf '## ADDED Requirements\n\n### Requirement: %s\n\nThe system SHALL do %s.\n\n#### Scenario: works\n\n- **WHEN** x\n- **THEN** y\n' "$3" "$3" > "openspec/changes/$1/specs/$2/spec.md"
}
# good_spec CAP REQ
good_spec() {
  mkdir -p "openspec/specs/$1"
  printf '# %s Specification\n\n## Purpose\n\nThis capability exists to describe the behavior of %s in enough words.\n\n## Requirements\n\n### Requirement: %s\n\nThe system SHALL do %s.\n\n#### Scenario: works\n\n- **WHEN** x\n- **THEN** y\n' "$1" "$1" "$2" "$2" > "openspec/specs/$1/spec.md"
}
run1() { # tool-label cmd...
  local label=$1; shift
  echo "### $label: ${*: -$(( $# > 6 ? 6 : $# ))}"
  "$@"; echo "[rc=$?]"
}
oracle() { echo "### ORACLE: $*"; "$O" "$@"; echo "[rc=$?]"; }
ospectra() { echo "### OPENSPECTRA: $*"; "$S" "$@"; echo "[rc=$?]"; }
openspec() { echo "### OPENSPEC: $*"; node "$OSJS" "$@"; echo "[rc=$?]"; }
