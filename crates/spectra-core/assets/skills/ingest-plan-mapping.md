For a plan-file source, map the parsed plan while preserving the selected change-id.

**Plan-to-Artifact Mapping**:

| Plan Section | Artifact | How to Map |
| --- | --- | --- |
| Title | Artifact title / summary | Preserve the selected change-id |
| Context | proposal: Why | Direct content transfer |
| Stages overview | proposal: What | Summarize all stages |
| Individual stages | tasks.md groups | One stage = one `##` heading, sub-items = `- [ ]` |
| File paths | proposal: Impact | Affected code list |
| Verification steps | tasks.md | Final verification task group |

