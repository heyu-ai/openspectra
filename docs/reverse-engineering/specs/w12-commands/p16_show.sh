#!/bin/bash
# show --deltas-only / -r / --item-type
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
mkjail p16
C=openspec/changes
mkchange ch
printf '## Why\n\nwhy text\n' > $C/ch/proposal.md
printf '## Decisions\n\n### D\n\nr\n' > $C/ch/design.md
printf '## 1. T\n\n- [ ] 1.1 x\n' > $C/ch/tasks.md
mkdir -p $C/ch/specs/cap-a $C/ch/specs/cap-b/sub
cat > $C/ch/specs/cap-a/spec.md <<'EOF'
## ADDED Requirements

### Requirement: Alpha

The system SHALL alpha.

#### Scenario: A1
- **WHEN** a
- **THEN** b

### Requirement: Beta

Beta text.

## MODIFIED Requirements

### Requirement: Gamma

Gamma changed.
EOF
printf '## REMOVED Requirements\n\n### Requirement: Old\n\n**Reason**: x\n' > $C/ch/specs/cap-b/spec.md
printf 'extra\n' > $C/ch/specs/cap-b/sub/notes.md
mkdir -p openspec/specs/cap-a openspec/specs/both
cat > openspec/specs/cap-a/spec.md <<'EOF'
# cap-a Specification

## Purpose

Purpose text.

## Requirements

### Requirement: Alpha

The system SHALL alpha.

#### Scenario: A1
- **WHEN** a
- **THEN** b

### Requirement: Zeta

Zeta.
EOF
printf 'extra spec file\n' > openspec/specs/cap-a/design-notes.md
# 同名：change 與 spec 都叫 both
mkchange both; printf '## Why\n\nchange both\n' > $C/both/proposal.md
printf '# both\n\n## Purpose\n\np\n\n## Requirements\n\n### Requirement: B\n\nb\n' > openspec/specs/both/spec.md
commit init
for args in "show ch --deltas-only" "show ch --deltas-only --json" "show ch -r" "show ch -r --json" "show ch --requirements --json" \
  "show cap-a -r" "show cap-a -r --json" "show cap-a --deltas-only" "show cap-a --deltas-only --json" \
  "show ch --item-type change" "show ch --item-type spec" "show cap-a --item-type spec" "show cap-a --item-type change" \
  "show both" "show both --item-type spec" "show both --item-type spec --json" "show both --item-type change --json" \
  "show ch --item-type bogus" "show ch --item-type Change" "show ch --item-type specs" "show --item-type spec" \
  "show ch --deltas-only -r" "show ch --deltas-only -r --json" "show ghost --deltas-only" "show ghost --item-type spec" "show ghost --item-type bogus" \
  "show ch --item-type" ; do
  oracle $args
done
