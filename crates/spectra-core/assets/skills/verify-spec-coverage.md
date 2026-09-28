Spec-coverage checks for verify. Fold findings into the parent's Completeness and Correctness dimensions.

Reuse complete test-scope content already loaded at the same version. Fetch `spectra instructions --skill test-scope` if absent, changed, unknown in origin/version, or incomplete after compaction. Classify each scenario and example into exactly one of three results: covered by a test, excluded by the test scope criterion, or an uncovered gap. For exclusions, report the ground for exclusion and omit it from warnings and test recommendations.

Coverage and traceability describe inspected assertions, not execution. Record execution evidence separately using the parent's four states, command, scope, identity and result source.

**Spec Coverage**:
- For each `### Requirement:` in `{{SPEC_DIR}}changes/<name>/specs/`, search for implementation evidence.
- If absent, add CRITICAL "Requirement not found: <requirement name>" and recommend "Implement requirement X: <description>".

**Requirement Implementation Mapping**:
- For each requirement, record implementation paths/lines and assess intent.
- For divergence, add WARNING "Implementation may diverge from spec: <details>" and recommend "Review <file>:<lines> against requirement X".

**Scenario Coverage**:
- For each `#### Scenario:`, check implementation and classify test coverage.
- For an uncovered gap, add WARNING "Scenario not covered: <scenario name>" and recommend "Add test or implementation for scenario: <description>".

**Example Traceability**:
- For each `##### Example:`, classify coverage; when covered, confirm the same GIVEN/WHEN/THEN values and every table row.
- For an uncovered gap, add WARNING "Spec example not covered by test: <example name>" and recommend a test using those values.

