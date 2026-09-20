Clarify ambiguities already identified by a parent workflow. This is an embedded helper, not a discovery workflow.

**Required parent input**

- The resolved change.
- The caller-provided findings. Each finding must include the file path and location, the concrete ambiguity, and its impact or evidence.

If required input is missing, name the missing parent input and stop without guessing. Do not resolve workflow identity, enumerate changes, or inspect unrelated artifacts.

**Workflow**

1. Deduplicate unresolved findings, rank them by impact, and select at most 3 highest-impact findings.
2. Ask one question at a time. Use structured input when available; otherwise present the same question in plain Markdown and wait for the answer.
3. Each question must:
   - identify the affected file and location;
   - provide 2-3 mutually exclusive options;
   - put the recommended option first and label it `(Recommended)`;
   - explain each option's artifact impact in one sentence.
4. After each answer, respect the selected option and do not re-ask it:
   - With edit access, make a minimal targeted edit to the existing artifact and briefly identify the changed wording.
   - Without edit access, provide the exact file and location plus before/after wording for the caller to apply.
5. Continue until the selected findings are resolved, the three-question cap is reached, or the user asks to stop. Then summarize only the decisions and edits made.

**Guardrails**

- Do not create artifacts or broaden an edit beyond the answered finding.
- Do not batch questions or infer an unanswered choice.
- Stop immediately if the user asks to skip clarification.

