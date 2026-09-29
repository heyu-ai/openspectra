# source me. Provides: mkjail NAME, $O (oracle), $R (openspectra), commit, run3, both
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@x GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@x
export GIT_AUTHOR_DATE="2026-09-01T00:00:00Z" GIT_COMMITTER_DATE="2026-09-01T00:00:00Z"
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w7gre
O=/Applications/Spectra.app/Contents/MacOS/spectra
R=/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w7f-custom-schema-apply/target/release/spectra
mkjail() {
  J=$W/jails/$1; rm -rf "$J"; mkdir -p "$J/openspec/changes/archive" "$J/openspec/specs"
  printf 'spec_dir: openspec\n' > "$J/.spectra.yaml"
  printf 'schema: spec-driven\n' > "$J/openspec/config.yaml"
  cd "$J" || exit 1
  git init -q -b main
}
commit() { git add -A && git -c commit.gpgsign=false commit -q -m "${1:-c}"; }
# run3 BIN args... : stdout / stderr / rc shown separately
run3() {
  local b=$1; shift
  "$b" "$@" >"$W/.o" 2>"$W/.e"; local rc=$?
  echo "--- stdout:"; cat "$W/.o"; echo "--- stderr:"; cat "$W/.e"; echo "--- rc=$rc"
}
both() { echo "### ORACLE: $*"; run3 "$O" "$@"; echo "### OPENSPECTRA: $*"; run3 "$R" "$@"; }
ora() { echo "### ORACLE: $*"; run3 "$O" "$@"; }
# mkschema NAME : copy of a valid minimal 2-artifact custom schema in openspec/schemas/NAME
mkschema() {
  local d=openspec/schemas/$1; mkdir -p "$d/templates"
  cat > "$d/schema.yaml" <<YAML
name: $1
version: 1
description: test schema
artifacts:
  - id: a
    generates: a.md
    description: A
    template: a.md
    instruction: do a
    requires: []
  - id: b
    generates: b.md
    description: B
    template: b.md
    instruction: do b
    requires:
      - a
apply:
  requires:
    - b
  tracks: b.md
  instruction: go
YAML
  printf '# A\n' > "$d/templates/a.md"; printf '# B\n' > "$d/templates/b.md"
}
