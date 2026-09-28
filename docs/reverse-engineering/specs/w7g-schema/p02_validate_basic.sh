#!/bin/bash
# p02: schema validate on valid schemas; default-name selection; --verbose; --json; outside a project
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh

echo "##### A: default config (schema: spec-driven), no NAME"
mkjail p02a; commit init
both schema validate
both schema validate --json
both schema validate --verbose
both schema validate --verbose --json
echo "## raw bytes of human output (piped)"
"$O" schema validate | od -c | head
both schema validate spec-driven
both schema validate no-spec
both schema validate no-spec --json
both schema validate no-spec --verbose

echo "##### B: config.yaml schema: mine (valid custom), no NAME"
mkjail p02b; mkschema mine; printf 'schema: mine\n' >| openspec/config.yaml; commit init
both schema validate
both schema validate --json
both schema validate --verbose
both schema validate mine --verbose --json

echo "##### C: config.yaml has no schema key, custom schema present"
mkjail p02c; mkschema mine; printf 'context: x\n' >| openspec/config.yaml; commit init
both schema validate
echo "##### C2: config.yaml absent"
rm openspec/config.yaml
both schema validate

echo "##### D: config says spec-driven, a change records schema: mine"
mkjail p02d; mkschema mine; mkdir -p openspec/changes/c1
printf 'schema: mine\ncreated: 2026-09-01\n' > openspec/changes/c1/.openspec.yaml; commit init
both schema validate

echo "##### E: config says no-spec"
mkjail p02e; printf 'schema: no-spec\n' >| openspec/config.yaml; commit init
both schema validate

echo "##### F: config says nosuch"
mkjail p02f; printf 'schema: nosuch\n' >| openspec/config.yaml; commit init
both schema validate
both schema validate --json

echo "##### G: two project schemas, config spec-driven (ours validates all project schemas?)"
mkjail p02g; mkschema s1; mkschema s2; commit init
both schema validate

echo "##### H: outside a project (plain git repo, no .spectra.yaml)"
J=$W/jails/p02h; rm -rf "$J"; mkdir -p "$J"; cd "$J"; git init -q -b main
both schema validate
both schema validate spec-driven --json
both schema validate nosuch
echo "##### H2: outside a project, openspec/ dir exists without .spectra.yaml"
mkdir -p openspec/schemas; cd openspec/..; mkschema mine
both schema validate mine
