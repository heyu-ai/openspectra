#!/bin/bash
# decisions：排序與日期來源
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
dz() { # change dir, heading
  mkdir -p "$1"; printf '## Decisions\n\n### %s\n\nr\n' "$2" > "$1/design.md"
}
mkjail p05
C=openspec/changes
# 同 created 日期、不同名稱
mkchange mmm; dz $C/mmm "H-mmm"
mkchange aaa; dz $C/aaa "H-aaa"
mkchange zzz; dz $C/zzz "H-zzz"
# 不同 created
mkdir -p $C/c-old; printf 'schema: spec-driven\ncreated: 2025-03-03\n' > $C/c-old/.openspec.yaml; dz $C/c-old "H-c-old"
mkdir -p $C/c-new; printf 'schema: spec-driven\ncreated: 2027-01-01\n' > $C/c-new/.openspec.yaml; dz $C/c-new "H-c-new"
# 無 .openspec.yaml
dz $C/c-noyaml "H-c-noyaml"
# created 缺
mkdir -p $C/c-nocreated; printf 'schema: spec-driven\n' > $C/c-nocreated/.openspec.yaml; dz $C/c-nocreated "H-c-nocreated"
# 封存：有日期前綴 / 無日期前綴 / 前綴與 created 不同
dz $C/archive/2026-05-05-arch-a "H-arch-a"
printf 'schema: spec-driven\ncreated: 2020-01-01\n' > $C/archive/2026-05-05-arch-a/.openspec.yaml
dz $C/archive/2026-05-05-arch-b "H-arch-b"
dz $C/archive/nodate-arch "H-nodate-arch"
printf 'schema: spec-driven\ncreated: 2024-04-04\n' > $C/archive/nodate-arch/.openspec.yaml
dz $C/archive/2026-13-45-badate "H-badate"
commit init
echo "## commit date 2026-09-01; today $(date +%F)"
oracle decisions
oracle decisions --json
echo "## touch mtimes to see if mtime matters"
touch -t 202001010000 $C/zzz/design.md
touch -t 203001010000 $C/aaa/design.md
oracle decisions
