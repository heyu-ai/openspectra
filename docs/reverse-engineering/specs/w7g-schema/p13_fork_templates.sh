#!/bin/bash
# p13: schema fork of a project schema whose single extra artifact 'b' has an unusual template; one jail per case
. /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/lib.sh
mk() { # $1 jail, $2 template value for b
  mkjail "$1"; mkschema m
  sed -i '' "s#    template: b.md#    template: $2#" openspec/schemas/m/schema.yaml
  rm openspec/schemas/m/templates/b.md
}
for who in oracle ours; do
  if [ $who = oracle ]; then B=$O; else B=$R; fi
  echo "################ $who"
  echo "##### empty template file"
  mk p13-empty-$who b.md; : > openspec/schemas/m/templates/b.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas/m2 -type f | sort; wc -c openspec/schemas/m2/templates/b.md

  echo "##### missing template"
  mk p13-missing-$who b.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas/m2 -type f | sort

  echo "##### traversal ../outside.md (exists at schemas/m/outside.md)"
  mk p13-trav-$who ../outside.md; printf 'OUT\n' > openspec/schemas/m/outside.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas -type f | sort

  echo "##### deep traversal ../../../../esc.md (exists relative to templates dir -> jail/esc.md? )"
  mk p13-trav2-$who ../../../../esc.md; printf 'ESC\n' > esc.md; commit init
  run3 "$B" schema fork m m2; git status --porcelain --untracked-files=all

  echo "##### subdir sub/deep.md"
  mk p13-sub-$who sub/deep.md; mkdir -p openspec/schemas/m/templates/sub; printf 'DEEP\n' > openspec/schemas/m/templates/sub/deep.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas/m2 -type f | sort

  echo "##### absolute template path (file in jail)"
  J=$W/jails/p13-abs-$who; mk p13-abs-$who "$J/abs.md"; printf 'ABS\n' > abs.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas/m2 -type f | sort; git status --porcelain --untracked-files=all; cat abs.md

  echo "##### two artifacts share one template"
  mk p13-share-$who a.md; commit init
  run3 "$B" schema fork m m2; find openspec/schemas/m2 -type f | sort
done
