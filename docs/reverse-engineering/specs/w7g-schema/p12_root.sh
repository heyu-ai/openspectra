#!/bin/bash
# p12: project-root discovery for `schema fork` / `schema validate` (oracle), all inside jails/
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
fresh() { J=$W/jails/$1; rm -rf "$J"; mkdir -p "$J/outer/inner/deeper"; cd "$J/outer" || exit 1; }
show() { (cd "$J" && find . -name schema.yaml | sort); }

echo "##### A: outer/openspec/ (bare dir, no .spectra.yaml), run in outer/inner/deeper"
fresh p12a; mkdir openspec; cd inner/deeper; ora schema fork no-spec r1; show
ora schema validate r1

echo "##### B: outer/.spectra.yaml spec_dir: docs/spec (no openspec dir), run in outer/inner"
fresh p12b; printf 'spec_dir: docs/spec\n' > .spectra.yaml; cd inner; ora schema fork no-spec r2; show

echo "##### C: outer/openspec + inner is its own git repo root"
fresh p12c; mkdir openspec; cd inner; git init -q -b main; ora schema fork no-spec r3; show

echo "##### D: outer/.spectra.yaml (spec_dir: openspec) + inner/openspec dir; run in inner"
fresh p12d; printf 'spec_dir: openspec\n' > .spectra.yaml; mkdir -p inner/openspec; cd inner; ora schema fork no-spec r4; show

echo "##### E: outer/.spectra.yaml spec_dir: docs + outer/openspec dir too; run in outer"
fresh p12e; printf 'spec_dir: docs\n' > .spectra.yaml; mkdir openspec docs; ora schema fork no-spec r5; show

echo "##### F: ours, case A layout"
fresh p12f; mkdir openspec; cd inner/deeper; run3 "$R" schema fork no-spec r1; show
