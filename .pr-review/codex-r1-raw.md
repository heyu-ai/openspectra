## Summary
The change correctly centralizes recursive delta collection for `validate` and `archive`, including symlink-cycle avoidance and deterministic ordering. I did not find merge-blocking issues in the diff.

## Findings

None.

## Verdict
- LGTM
