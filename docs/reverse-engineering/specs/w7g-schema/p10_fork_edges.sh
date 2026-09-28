#!/bin/bash
# p10: schema fork edge cases (oracle; ours shown for the error-path cases)
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
tree() { find "$1" -print | sort; }

echo "##### A: --force onto an existing different target with extra files"
mkjail p10a; mkschema t; printf 'extra\n' > openspec/schemas/t/EXTRA.txt; printf 'old\n' > openspec/schemas/t/templates/old.md; commit init
ora schema fork no-spec t
ora schema fork no-spec t --force
tree openspec/schemas/t; head -3 openspec/schemas/t/schema.yaml
echo "## --force when target absent"; ora schema fork no-spec fresh --force

echo "##### B: self --force rewrites schema.yaml?"
mkjail p10b; mkschema m; printf '# c\n' >> openspec/schemas/m/schema.yaml; printf 'keep\n' > openspec/schemas/m/K.txt; commit init
ora schema fork m m --force; tree openspec/schemas/m; cat openspec/schemas/m/schema.yaml; git status --porcelain

echo "##### C: source template missing / empty / traversal / subdir / absolute; optional fields absent"
mkjail p10c; mkschema m
cat > openspec/schemas/m/schema.yaml <<'YAML'
name: m
version: 7
artifacts:
  - id: a
    generates: a.md
    description: A
    template: a.md
    instruction: ia
  - id: b
    generates: b.md
    description: B
    template: missing.md
    instruction: ib
  - id: c
    generates: c.md
    description: C
    template: ../outside.md
    instruction: ic
  - id: d
    generates: d.md
    description: D
    template: sub/deep.md
    instruction: id
  - id: e
    generates: e.md
    description: E
    template: /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/jails/p10c/abs.md
    instruction: ie
  - id: f
    generates: f.md
    description: F
    template: empty.md
    instruction: if
apply:
  requires: []
YAML
printf 'OUT\n' > openspec/schemas/outside.md; mkdir -p openspec/schemas/m/templates/sub; printf 'DEEP\n' > openspec/schemas/m/templates/sub/deep.md
printf 'ABS\n' > abs.md; : > openspec/schemas/m/templates/empty.md; commit init
ora schema validate m
ora schema fork m m2
echo "## tree"; tree openspec/schemas; echo "## m2/schema.yaml"; cat openspec/schemas/m2/schema.yaml
echo "## outside.md after"; cat openspec/schemas/outside.md; git status --porcelain

echo "##### D: invalid project source / nosuch / not-parseable"
mkjail p10d; mkschema bad; sed -i '' 's/      - a$/      - zzz/' openspec/schemas/bad/schema.yaml
mkschema unp; printf 'name: [\n' >| openspec/schemas/unp/schema.yaml; commit init
both schema fork bad bad2
both schema fork unp unp2
both schema fork nosuch x
both schema fork nosuch x --json
ls openspec/schemas

for who in oracle ours; do
  if [ $who = oracle ]; then B=$O; else B=$R; fi
  echo "################################ $who"
  echo "##### E: bad target names"
  mkjail p10e-$who; commit init
  for n in "../esc" "a/b" "" ".hidden" "UPPER" "x y" "spec-driven" "no-spec" "-dash" "x.yaml"; do echo "## target [$n]"; run3 "$B" schema fork no-spec -- "$n"; done
  echo "## tree after"; find openspec/schemas "$W/jails/p10e-$who/openspec/esc" -print 2>/dev/null | sort
  echo "## schemas --json names"; "$B" schemas --json | python3 -c "import json,sys; print([s['name'] for s in json.load(sys.stdin)])"

  echo "##### F: target exists as a regular file"
  mkjail p10f-$who; mkdir -p openspec/schemas; printf 'f\n' > openspec/schemas/tf; commit init
  run3 "$B" schema fork no-spec tf
  run3 "$B" schema fork no-spec tf --force
  find openspec/schemas -print | sort

  echo "##### G: outside a project"
  J=$W/jails/p10g-$who; rm -rf "$J"; mkdir -p "$J"; cd "$J"; git init -q -b main
  run3 "$B" schema fork no-spec g1; find . -path ./.git -prune -o -print | sort

  echo "##### H: custom spec_dir"
  mkjail p10h-$who; rm -rf openspec; mkdir -p docs/spec/changes/archive docs/spec/specs; printf 'spec_dir: docs/spec\n' >| .spectra.yaml; commit init
  run3 "$B" schema fork no-spec h1; find . -path ./.git -prune -o -name schema.yaml -print | sort
done
