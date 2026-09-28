#!/bin/bash
# W9 p08：每條規則一個合成 change／spec，三個工具都跑 --changes/--specs --json（非 strict）
source /Users/howie/.claude/jobs/9eb90dff/tmp/w9re/lib.sh
mkjail p08
C=openspec/changes
SP=openspec/specs
REQ='### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
# main specs the MODIFIED cases target
mkdir -p $SP/base
printf '# base\n\n## Purpose\n\nThe base capability holds requirements that changes modify in probes.\n\n## Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n\n#### Scenario: a2\n\n- **WHEN** x\n- **THEN** y\n' > $SP/base/spec.md
mkdir -p $SP/broken-main
printf '# bm\n\n## Purpose\n\nA main spec that contains a pasted delta header after its requirements.\n\n## Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n\n## ADDED Requirements\n\n### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b1\n\n- **WHEN** x\n- **THEN** y\n' > $SP/broken-main/spec.md

d() { # name cap content
  mkchange "$1"; mkdir -p "$C/$1/specs/$2"; printf "$3" > "$C/$1/specs/$2/spec.md"
}
mkchange d01-good; good_delta d01-good newcap "Good"
mkchange d02-nospecs
mkchange d03-noops; mkdir -p $C/d03-noops/specs/x; printf '# notes\n\nno delta here\n' > $C/d03-noops/specs/x/spec.md
mkchange d04-rootspec; mkdir -p $C/d04-rootspec/specs; printf "## ADDED Requirements\n\n$REQ" > $C/d04-rootspec/specs/spec.md
d d05-dupadded x "## ADDED Requirements\n\n$REQ\n$REQ"
d d06-addrem x "## ADDED Requirements\n\n$REQ\n## REMOVED Requirements\n\n### Requirement: Alpha\n"
d d07-noscenario x '## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n'
d d08-notext x '## ADDED Requirements\n\n### Requirement: Alpha\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
d d09-noshall x '## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system does alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
d d10-modmissing base '## MODIFIED Requirements\n\n### Requirement: Nope\n\nThe system SHALL nope.\n\n#### Scenario: n1\n\n- **WHEN** x\n- **THEN** y\n'
d d11-modnomain nomain "## MODIFIED Requirements\n\n$REQ"
d d12-modloss base "## MODIFIED Requirements\n\n$REQ"
d d13-strayh3 x "## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n### Notes\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n"
d d14-noname x '## ADDED Requirements\n\n### Requirement:\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
d d15-fromonly base '## RENAMED Requirements\n\n- FROM: `### Requirement: Alpha`\n'
d d16-twoadded x "## ADDED Requirements\n\n$REQ\n## ADDED Requirements\n\n### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b1\n\n- **WHEN** x\n- **THEN** y\n"
d d17-emptyadded x '## ADDED Requirements\n\nnothing here\n'
mkchange d18-capmd; mkdir -p $C/d18-capmd/specs; printf "## ADDED Requirements\n\n$REQ" > $C/d18-capmd/specs/x.md
mkchange d19-skipspecs; printf 'schema: spec-driven\ncreated: 2026-09-01\nskip_specs: true\n' > $C/d19-skipspecs/.openspec.yaml
d d20-addexisting base "## ADDED Requirements\n\n$REQ"
d d21-remmissing base '## REMOVED Requirements\n\n### Requirement: Nope\n'
d d22-modbrokenmain broken-main "## MODIFIED Requirements\n\n$REQ"
d d23-mixed base '## MODIFIED Requirements\n\n### Requirement: Nope\n\nThe system SHALL nope.\n\n#### Scenario: n1\n\n- **WHEN** x\n- **THEN** y\n'
mkdir -p $C/d23-mixed/specs/zz; printf '# notes only\n' > $C/d23-mixed/specs/zz/spec.md
mkchange d24-noproposal; good_delta d24-noproposal newcap2 "Good2"; rm $C/d24-noproposal/proposal.md
d d25-emptyscenario x '## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n'
d d26-h5scenario x '## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n##### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
d d27-orphan x "## Notes\n\n$REQ"
d d28-shallheader x '## ADDED Requirements\n\n### Requirement: The system SHALL alpha\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
d d29-dupscenario base '## MODIFIED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n'
mkchange d30-tasks; good_delta d30-tasks newcap3 "Good3"
printf '## 1. Group\n\n- [ ] 1.1 a\n- [ ] 1.1 b\n- [ ] 2.1 c\n' > $C/d30-tasks/tasks.md

s() { mkdir -p "$SP/$1"; printf "$2" > "$SP/$1/spec.md"; }
P='## Purpose\n\nThis capability exists to describe behavior in a sufficiently long sentence.\n\n'
s s01-good "# s\n\n$P## Requirements\n\n$REQ"
s s02-nopurpose "# s\n\n## Requirements\n\n$REQ"
s s03-emptypurpose "# s\n\n## Purpose\n\n## Requirements\n\n$REQ"
s s04-tbdpurpose "# s\n\n## Purpose\n\nTBD - created by archiving change foo. Update Purpose after archive.\n\n## Requirements\n\n$REQ"
s s05-noreqsection "# s\n\n$P"
s s06-emptyreqs "# s\n\n$P## Requirements\n\nNothing yet.\n"
s s07-plainh3 "# s\n\n$P## Requirements\n\n### Admin Portal\n\nThe system SHALL admin.\n\n#### Scenario: p1\n\n- **WHEN** x\n- **THEN** y\n"
s s08-noscenario "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n"
s s09-notext "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n"
s s10-noshall "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\nThe system does alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n"
s s11-deltaheader "# s\n\n$P## Requirements\n\n$REQ\n## MODIFIED Requirements\n\n### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b1\n\n- **WHEN** x\n- **THEN** y\n"
s s12-dupreq "# s\n\n$P## Requirements\n\n$REQ\n$REQ"
s s13-outside "# s\n\n$P## Requirements\n\n$REQ\n## Notes\n\n### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b1\n\n- **WHEN** x\n- **THEN** y\n"
s s14-noname "# s\n\n$P## Requirements\n\n### Requirement:\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n"
s s15-shortpurpose "# s\n\n## Purpose\n\nShort.\n\n## Requirements\n\n$REQ"
LONG=$(python3 -c 'print("The system SHALL " + "x" * 520 + ".")')
s s16-longtext "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\n$LONG\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n"
s s17-h5scenario "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n##### Scenarios:\n\n- **WHEN** x\n- **THEN** y\n"
s s18-emptyscenario "# s\n\n$P## Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a1\n"
s s19-todopurpose "# s\n\n## Purpose\n\nTODO: write the purpose of this capability properly later on please.\n\n## Requirements\n\n$REQ"
s s20-h3purpose "# s\n\n### Purpose\n\nThis capability exists to describe behavior in a sufficiently long sentence.\n\n## Requirements\n\n$REQ"
commit init

OUT=$W9/p08
mkdir -p "$OUT"
export NO_COLOR=1
for scope in changes specs; do
  "$O" validate --$scope --json > "$OUT/$scope.oracle.json" 2> "$OUT/$scope.oracle.err"; echo "oracle $scope rc=$?"
  "$O" validate --$scope > "$OUT/$scope.oracle.txt" 2>&1; echo "oracle human $scope rc=$?"
  "$S" validate --$scope --json > "$OUT/$scope.os.json" 2> "$OUT/$scope.os.err"; echo "openspectra $scope rc=$?"
  node "$OSJS" validate --$scope --json --no-interactive > "$OUT/$scope.openspec.json" 2> "$OUT/$scope.openspec.err"; echo "openspec $scope rc=$?"
done
