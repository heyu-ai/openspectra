#!/bin/bash
# p03: unknown schema names, stream split, --json/--verbose on failure, odd names, custom spec_dir, shadowing
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
mkjail p03a; commit init
both schema validate nosuch
both schema validate nosuch --json
both schema validate nosuch --verbose
both schema validate nosuch --verbose --json
both schema validate ../x
both schema validate "a/b"
both schema validate ""
both schema validate Spec-Driven
both schema validate spec-driven-custom

echo "##### B: spec_dir: docs/spec (custom) with a project schema there"
mkjail p03b; rm -rf openspec; mkdir -p docs/spec/changes/archive docs/spec/specs
printf 'spec_dir: docs/spec\n' >| .spectra.yaml; printf 'schema: spec-driven\n' > docs/spec/config.yaml
mkdir -p docs/spec/schemas; cd docs/spec/..; cd ..
mkschema tmp1; mv openspec/schemas/tmp1 docs/spec/schemas/mine; sed -i '' 's/^name: tmp1/name: mine/' docs/spec/schemas/mine/schema.yaml; rm -rf openspec
commit init
both schema validate mine
echo "## and a schema only under openspec/schemas (not the configured spec_dir)"
mkschema other; commit o
both schema validate other

echo "##### C: project schema named spec-driven that is INVALID shadows builtin?"
mkjail p03c; mkschema spec-driven
sed -i '' 's/      - a$/      - zzz/' openspec/schemas/spec-driven/schema.yaml
commit init
both schema validate
both schema validate spec-driven
both schema validate spec-driven --json
"$O" schema which spec-driven

echo "##### D: project schema named no-spec (valid, 2 artifacts) shadows builtin?"
mkjail p03d; mkschema no-spec; commit init
both schema validate no-spec
