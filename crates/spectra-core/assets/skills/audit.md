Apply this condensed security discipline continuously inside the already-authorized implementation task. Do not collect a standalone diff or launch a separate audit workflow.

## Quick 3-Role Check

Before finalizing an API, configuration option, parameter, or security boundary, check:

1. **Scoundrel**: Can this be abused? Can configuration disable security? Can values be injected or algorithms downgraded?
2. **Lazy Developer**: Is the default safe? Is copy-paste usage secure? Do errors guide the developer toward the safe path?
3. **Confused Developer**: Can parameters be swapped? Does wrong usage fail loudly? Are security values represented by distinct semantic types?

## Implementation Red Flags

- String parameters for security choices: prefer an enum or newtype.
- Configuration that defaults to an unsafe off state: make the safe path the default.
- Ambiguous zero, nil, or empty values: define and validate their meaning.
- Ignorable boolean security checks: return a result that callers must handle.
- Arbitrary algorithm or mode selection: constrain choices to safe values.
- Unvalidated configuration: reject invalid or malicious combinations loudly.

If one of these sharp edges appears within the current task, correct it as part of that authorized implementation and verify the failure path with a test.

