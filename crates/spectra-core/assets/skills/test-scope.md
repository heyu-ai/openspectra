## Test scope

Before writing an assertion, ask one question: **does this contract cross the places that use it?**

- **It crosses** — write the test: a break surfaces at a location you are not currently looking at. This includes the rendered class contract of a design system primitive reused across the application (Button, Input, Card, Badge, nav tab).
- **It does not cross** — leave it untested: a break is visible at the point of change. This includes single-surface corner radius, padding, centering, dimensions and spacing.

Keep a conditional branch, a default-value contract, or an error-degradation path in scope whatever layer it sits in, including delegation: test each branch and test the degraded outcome. "unimportant" and "effort" are invalid grounds.

Apply the criterion before writing the assertion.

Report exclusion grounds. The criterion settles only whether an assertion is written.


