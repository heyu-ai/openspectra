#!/bin/bash
# decisions：日期來源細節、active 名稱前綴、parked、錯誤情境
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"; }
mkjail p14
C=openspec/changes
mkdir -p $C/y-createdonly; printf 'created: 2026-02-10\n' > $C/y-createdonly/.openspec.yaml; dz $C/y-createdonly "createdonly"
mkdir -p $C/y-ts; printf 'schema: spec-driven\ncreated: 2026-02-10T10:00:00Z\n' > $C/y-ts/.openspec.yaml; dz $C/y-ts "ts"
mkdir -p $C/y-num; printf 'schema: spec-driven\ncreated: 20260210\n' > $C/y-num/.openspec.yaml; dz $C/y-num "num"
mkdir -p $C/y-quoted; printf "schema: spec-driven\ncreated: '2026-02-11'\n" > $C/y-quoted/.openspec.yaml; dz $C/y-quoted "quoted"
mkdir -p $C/y-broken; printf 'schema: [unclosed\n' > $C/y-broken/.openspec.yaml; dz $C/y-broken "broken"
mkdir -p $C/y-free; printf 'schema: spec-driven\ncreated: yesterday\n' > $C/y-free/.openspec.yaml; dz $C/y-free "free"
mkchange 2026-05-05-activeprefixed; dz $C/2026-05-05-activeprefixed "activeprefixed"
mkdir -p $C/archive/2026-13-01-arch; printf 'schema: spec-driven\ncreated: 2020-01-01\n' > $C/archive/2026-13-01-arch/.openspec.yaml; dz $C/archive/2026-13-01-arch "arch-invalid-month-prefix"
mkdir -p $C/archive/2026-x-arch-yaml; printf 'schema: spec-driven\ncreated: 2021-01-01\n' > $C/archive/2026-x-arch-yaml/.openspec.yaml; dz $C/archive/2026-x-arch-yaml "arch-bad-prefix-with-yaml"
mkdir -p $C/archive/2026-x-arch-cronly; printf 'created: 2021-02-02\n' > $C/archive/2026-x-arch-cronly/.openspec.yaml; dz $C/archive/2026-x-arch-cronly "arch-bad-prefix-createdonly"
# design.md 是目錄、非 UTF-8
mkchange y-dirdesign; mkdir -p $C/y-dirdesign/design.md
mkchange y-latin1; printf '## Decisions\n\n### Caf\xe9\n\nr\n' > $C/y-latin1/design.md
# 檔案（非目錄）在 changes/ 下
printf 'x\n' > $C/stray.md
commit init
oracle decisions --json
oracle decisions
echo "## parked"
mkjail p14p; mkchange pk; dz openspec/changes/pk "parked-one"; mkchange keep; dz openspec/changes/keep "kept"; commit init
oracle park pk
oracle list --parked
oracle decisions
echo "## no changes dir"
mkjail p14n; rm -rf openspec/changes; oracle decisions; oracle decisions --json
echo "## only archive dir missing"
mkjail p14m; rm -rf openspec/changes/archive; mkchange a; dz openspec/changes/a "A"; oracle decisions
echo "## archive/ contains a file and a dir without design"
mkjail p14f; printf 'x' > openspec/changes/archive/file.md; mkdir -p openspec/changes/archive/2026-01-01-nodesign; oracle decisions
echo "## subdirectory cwd"
mkjail p14s; mkchange a; dz openspec/changes/a "A"; mkdir -p deep/er; cd deep/er; oracle decisions
