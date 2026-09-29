## Context

<!-- Background and current state -->

## Design Source

<!--
Include this section ONLY when the change originates from a visual design source
(a Claude Design handoff, .dc.html, bundle, or design screenshot). Omit it
entirely for changes with no visual design source — same conditional rule as the
Implementation Contract section below.

Record:
- Source: which .dc.html / handoff, and when it was obtained
- Cache: the local path under .spectra/design-cache/<change-name>/ that holds
  the original .dc.html (gitignored, never committed)
- Key values: a table of the exact px / hex / radius / spacing / icon names read
  from the .dc.html, so apply can reproduce the design pixel-by-pixel
-->

## Goals / Non-Goals

**Goals:**

<!-- What this design aims to achieve -->

**Non-Goals:**

<!-- What is explicitly out of scope -->

## Decisions

<!-- Key design decisions and rationale.

Each decision is a `###` heading; `spectra decisions` lists them across changes,
so the heading text is what a reader sees when scanning for a past decision.

When a decision replaces an earlier one, declare it directly under the heading:

    **Supersedes**: <change-name> / <decision heading>

Use the change name without its archive date prefix. The field is optional, and
supersession is never inferred — two decisions on the same subject stay
unrelated unless one of them says otherwise.
-->

## Implementation Contract

<!--
Required for changes that create or modify behavior. Skip only for pure
artifact / documentation cleanup with no runtime or tooling effect.

Cover the durable handoff to apply:
- Behavior: what an end user, caller, or operator observes once this change ships
- Interface / data shape: command names, function signatures, JSON shapes,
  IPC contracts, file formats — name them, do not reference line numbers
- Failure modes: error shapes, fallback behavior, what is intentionally
  silent vs. surfaced
- Acceptance criteria: how an implementer or reviewer can confirm the
  contract is satisfied (tests, CLI invocations, analyzer checks, manual
  verification)
- Scope boundaries: what is explicitly in scope and what is out — keeps
  apply from drifting into adjacent work

File paths are supporting context for locating the work; they are never
the contract itself.
-->

## Risks / Trade-offs

<!-- Known risks and trade-offs -->
