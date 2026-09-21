Follow Test-Driven Development discipline for the parent-provided task only.

## Parent contract

The parent supplies task, scope, contract/spec excerpts and verification commands. Do not select a change or broaden scope. Missing/contradictory input: report and stop without guessing.

## The Iron Law

Never write implementation code unless a failing test demands it. Exceptions: existing behavior uses Regression; a Test scope exclusion is the second exception to the Iron Law, using Excluded workflow.

## Test scope

Before writing an assertion, ask one question: **does this contract cross the places that use it?**

- **It crosses** — write the test: a break surfaces at a location you are not currently looking at. This includes the rendered class contract of a design system primitive reused across the application (Button, Input, Card, Badge, nav tab).
- **It does not cross** — leave it untested: a break is visible at the point of change. This includes single-surface corner radius, padding, centering, dimensions and spacing.

Keep a conditional branch, a default-value contract, or an error-degradation path in scope whatever layer it sits in, including delegation: test each branch and test the degraded outcome. "unimportant" and "effort" are invalid grounds.

Apply the criterion before writing the assertion.

Report exclusion grounds. The criterion settles only whether an assertion is written.

## Excluded workflow

For an excluded property, implementation without a failing test is allowed. Record the excluded property and its ground, then implement the change without adding an assertion for that property. Run the task's existing relevant automated checks; compare the changed surface with the task or design source when possible, otherwise report visual verification as pending. Other behavior uses Red-Green-Refactor.

## Expected values

Expected values come from a known-good literal, a concrete example in the spec, or a derivation independent of the implementation. Two forms fail this rule: calling the code under test for the expected value, and repeating the production computation inside the test. When no independent source exists, report the gap to the parent.

The Mutation check misses copied computations: they can survive a production break. Independent expected values cover that gap.

## Red-Green-Refactor

Repeat a small cycle for each behavior in the task:

1. **RED** — Write the smallest focused test; retain command and intended behavioral failure as evidence. Expected behavioral RED proceeds to GREEN. Setup/syntax failure is not RED: repair in-scope causes and retry; use the parent's bounded failure policy for unresolved blockers.
2. **GREEN** — Make the minimum change. Run the focused test, then the relevant suite. Keep the passing results as GREEN evidence.
3. **REFACTOR** — Improve structure only while green. Re-run the focused test after each step, and the suite before finishing.

If a new test passes immediately, do not claim RED. Determine whether the behavior is already implemented, the assertion is weak, or the test misses the production path. Use the Regression workflow when existing behavior is intentional.

## Spec mapping

Treat each relevant spec scenario as required behavior. When the parent provides a `##### Example:` block, preserve its exact values and map it directly:

- GIVEN → setup and preconditions
- WHEN → action
- THEN → observable assertions
- Table rows → parameterized cases

In-scope examples are minimum coverage; also test relevant error paths and boundaries.

## Bug Fix workflow

1. Write a focused test reproducing the bug through the real behavior path.
2. Confirm the failure represents the bug.
3. Apply the smallest fix.
4. Run the reproducer and relevant suite to green.

Never fix an in-scope bug without a reproducing test. If reproduction is impossible with the supplied context, return the blocker to the parent instead of guessing.

## Regression workflow

Use this when protected behavior already passes before the new test exists:

1. Trace the relevant inputs, outputs, and side effects.
2. Add focused assertions for the current behavior and confirm they pass.
3. **Mutation check:** deliberately break the protected production behavior; the new test MUST fail for that break.
4. Restore the implementation immediately and confirm the focused test passes again.

Do not leave the deliberate mutation in the working tree. If the mutation does not fail, strengthen the test before continuing.

## Completion evidence

Return task-scoped evidence:

- touched test and implementation files;
- RED evidence, a Mutation check failure, or the test-scope exclusion ground;
- GREEN evidence, or existing checks and visual status for an excluded property;
- unresolved blockers or unverified requirements.

Never skip, disable, or weaken a failing test to obtain green. Complete the task only when implementation and tests are restored to a passing state.

