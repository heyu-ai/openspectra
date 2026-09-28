#!/bin/bash
# p09: schema fork of PROJECT schemas; copy-vs-reserialize; broken sources; oracle vs ours
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
setup() { # $1 jail name
  mkjail "$1"; mkschema m
  # decorate the source: comment, unknown key, missing artifact instruction, flow style, extra files
  cat > openspec/schemas/m/schema.yaml <<'YAML'
# leading comment
name: m   # trailing comment
version: 1
description: test schema
custom_top: keep-me
artifacts:
  - {id: a, generates: a.md, description: A, template: a.md, requires: []}
  - id: b
    generates: b.md
    description: B
    template: b.md
    instruction: do b
    requires: [a]
    extra_key: 1
apply:
  requires: [b]
  tracks: b.md
YAML
  printf 'notes\n' > openspec/schemas/m/NOTES.txt
  printf 'hidden\n' > openspec/schemas/m/.hidden
  mkdir -p openspec/schemas/m/templates/sub; printf 'x\n' > openspec/schemas/m/templates/sub/x.md
  commit init
}
for who in oracle ours; do
  if [ $who = oracle ]; then B=$O; else B=$R; fi
  echo "################ $who"
  setup p09-$who
  echo "## fork m m2"; run3 "$B" schema fork m m2
  echo "## tree m2"; find openspec/schemas/m2 -print | sort
  echo "## m2/schema.yaml"; cat openspec/schemas/m2/schema.yaml
  echo "## cmp with source"; cmp openspec/schemas/m/schema.yaml openspec/schemas/m2/schema.yaml && echo IDENTICAL
  echo "## fork m (default name)"; run3 "$B" schema fork m
  echo "## fork m m (onto itself)"; run3 "$B" schema fork m m
  echo "## fork m m --force (onto itself)"; run3 "$B" schema fork m m --force
  echo "## tree m after self-force"; find openspec/schemas/m -print | sort
  echo "## fork m2 m2b: then m2b validate"; run3 "$B" schema fork m2 m2b; run3 "$B" schema validate m2b
done
