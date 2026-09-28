#!/bin/bash
# decisions：基本列出、JSON、keyword、supersession
. /Users/howie/.claude/jobs/9eb90dff/tmp/w12/lib.sh
mkjail p04
oracle decisions
oracle decisions --json
mkchange alpha
cat > openspec/changes/alpha/design.md <<'EOF'
## Context

ctx

### Not A Decision In Context

text

## Decisions

### Use SQLite

We pick SQLite because it is embedded.
Second line of rationale.

#### Sub heading

sub text

### Empty Rationale

### Replace Cache

**Supersedes**: old-change / Use Redis

We now use an in-process cache.

### Bad Ref

**Supersedes**: ghost / Nothing

Pointing nowhere.

## Risks / Trade-offs

### Not A Decision In Risks

r
EOF
mkdir -p openspec/changes/archive/2026-01-15-old-change
printf 'schema: spec-driven\ncreated: 2026-01-10\n' > openspec/changes/archive/2026-01-15-old-change/.openspec.yaml
cat > openspec/changes/archive/2026-01-15-old-change/design.md <<'EOF'
## Decisions

### Use Redis

Redis is fast.

### Keep Logs

Logs are kept for 30 days.
EOF
mkchange beta
cat > openspec/changes/beta/design.md <<'EOF'
## Decisions

### Beta Choice

Beta rationale mentions sqlite lowercase.
EOF
mkchange gamma
printf '## Why\n\nno design\n' > openspec/changes/gamma/proposal.md
commit init
oracle decisions
oracle decisions --json
oracle decisions sqlite
oracle decisions SQLite
oracle decisions Redis
oracle decisions "Use"
oracle decisions nomatch
oracle decisions nomatch --json
oracle decisions days --json
oracle decisions --no-color
oracle decisions a b
