#!/usr/bin/env python3
"""Verify or recapture the `spectra analyze` golden from the 3.0.0 oracle.

This is a verification contract, not a printer. Every scenario below is built
as a fresh scratch project (no git: `analyze` does not need one) and each run
records the oracle's exit code, stdout and stderr. Runs marked ``tty`` execute
with stdout on a pseudo-terminal, so the colour contract is captured from a
real terminal rather than from ``CLICOLOR_FORCE``; their ``\\r\\n`` is folded
back to ``\\n``. By default the capture is compared against the committed
golden and any drift exits non-zero, keeping the scratch projects for
inspection. ``--write`` captures a candidate, recaptures it in a second
scratch dir, and replaces the golden only when both captures agree
byte-for-byte.

Two oracle behaviours cannot be reproduced byte-for-byte and are normalised
here, each a deliberate OpenSpectra divergence documented in
`docs/reverse-engineering/analyze.md`:

- `params` objects are serialised from a hash map, so multi-key params come
  out in a random order that changes between runs. The capture re-emits every
  JSON stdout with each `params` object's keys sorted, after checking that
  the re-emitter reproduces the oracle's bytes exactly when nothing is
  reordered.
- Spec directories and change directories are visited in `readdir` order.
  OpenSpectra visits them in byte-sorted name order, so the capture fails if
  a directory whose order is observable lists in a different order; rename
  the fixture's directories until it does.

The golden is self-describing (it carries every project's files), so
`crates/spectra-cli/tests/analyze_golden_integration.rs` replays it against
OpenSpectra without re-deriving the fixtures.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
itself overrides the standard application path.
"""

import argparse
import json
import os
import pty
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
DEFAULT_BIN = "/Applications/Spectra.app/Contents/MacOS/spectra"
GOLDEN_REL = Path("docs/reverse-engineering/golden/analyze-3.0.0.json")
# conNumericClaimMismatch 的案例（W10 探測 p40–p59，每個案例一個 change 的 proposal／
# design／tasks，`null` 代表該檔不存在）。規則的探測紀錄見 analyze.md。
NUMERIC_CASES = json.loads((Path(__file__).resolve().parent / "capture-analyze-numeric-cases.json").read_text())


def _numeric_pair(proposal_line: str, design_line: str) -> dict:
    return {"proposal": proposal_line + "\n", "design": design_line + "\n", "tasks": "- [ ] 1.1 x\n"}


# W10 實作時補的案例：轉折（→）後的結構詞判斷、非 ASCII 字母單位。
NUMERIC_CASES.update(
    {
        "x1-phase-transition": _numeric_pair("aaaa abbb accc addd phase 3 → 5", "aaaa abbb accc addd phase 4 → 6"),
        "x2-banben-not-last": _numeric_pair("aaaa abbb accc addd 版本 x 3", "aaaa abbb accc addd 版本 x 4"),
        "x3-cjk-after-number": _numeric_pair("aaaa abbb accc addd 3個", "aaaa abbb accc addd 4個"),
        "x4-ascii-arrow-phase": _numeric_pair("aaaa abbb accc addd phase 3 -> 5", "aaaa abbb accc addd phase 4 -> 6"),
        "x5-units-in-transition": _numeric_pair("aaaa abbb accc addd step 3 ms → 5 ms", "aaaa abbb accc addd step 4 ms → 6 ms"),
        "x6-to-not-arrow": _numeric_pair("aaaa abbb accc addd step 3 to 5", "aaaa abbb accc addd step 4 to 6"),
        "x7-arrow-no-space": _numeric_pair("aaaa abbb accc addd phase 3→5", "aaaa abbb accc addd phase 4→6"),
        "x8-crlf": _numeric_pair("aaaa abbb accc addd 3\r", "aaaa abbb accc addd 4\r"),
    }
)
SPEC_DIR = "openspec"

BASE_FILES = {
    ".spectra.yaml": f"spec_dir: {SPEC_DIR}\n",
    f"{SPEC_DIR}/config.yaml": "schema: spec-driven\n",
    f"{SPEC_DIR}/specs/.gitkeep": "",
    f"{SPEC_DIR}/changes/archive/.gitkeep": "",
}
OPENSPEC_YAML = "schema: spec-driven\ncreated: 2026-01-01\n"

# Shared fragments ------------------------------------------------------------

SC = "#### Scenario: s\n\n- **GIVEN** g\n"
SC_ABSTRACT = "#### Scenario: s\n\n- **WHEN** w\n"


def req(name: str, body: str = "The system SHALL work.\n", scenario: str = SC) -> str:
    return f"### Requirement: {name}\n\n{body}\n{scenario}\n"


def ch(change: str, rel: str) -> str:
    return f"{SPEC_DIR}/changes/{change}/{rel}"


def main_spec(cap: str) -> str:
    return f"{SPEC_DIR}/specs/{cap}/spec.md"


def json_run(change: str) -> dict:
    return {"args": ["analyze", change, "--json"]}


def human_run(change: str) -> dict:
    return {"args": ["analyze", change]}


def both(*changes: str) -> list:
    return [run for c in changes for run in (json_run(c), human_run(c))]


CAPABILITIES_PROPOSAL = """## Why

x

## Capabilities

### New Capabilities

- `cap-ok`: present
- `cap-miss`: bullet missing
- `Has Space`: spaced
- `<name>`: placeholder
- `Upper_Case`: upper underscore
- `nested/cap`: slash
- plain bullet no backtick
- two `first-tok` and `second-tok`
continuation line `cont-tok` here
| `table-tok` | ADDED |
* `star-tok`: star bullet
  - `indented-tok`: indented
- `with.dot`: dot
- `trailing-`: dash
- ``: empty
- `i\t2`: tab inside
- `cap-ok`: duplicate present
- `cap-miss`: duplicate missing
- `x`: single

#### Deeper heading keeps the section open

- `h4-tok`: after h4

```
- `fenced-tok`: fenced bullet
## Fenced heading ends the section
- `after-fence`: not extracted
```

## Capabilities (v2)

- `reopened-tok`: suffix heading reopens

## What Changes

- `after-section`: not in caps
"""

TASK_LINES_SPEC = (
    "## Purpose\n\nP.\n\n## ADDED Requirements\n\n"
    + "".join(
        req(n, "SHALL.\n")
        for n in [
            "InCheckbox",
            "InHeading",
            "InContinuation",
            "InPlainLine",
            "InDoneBox",
            "InPlainBullet",
            "CaseMix",
            "Partial Name Here",
            "StarBox",
            "PlusBox",
            "TildeBox",
            "NoSpaceBox",
            "NumberedBox",
            "TabBullet",
            "IndentedBox",
            "UpperXBox",
            "LinkReq",
            "LetterReq",
            "EmptyBracketReq",
            "Twice",
            "Twice",
        ]
    )
    + "## MODIFIED Requirements\n\n"
    + req("ModNoTask", "SHALL.\n")
    + "## REMOVED Requirements\n\n### Requirement: RemovedNoTask\n\n**Reason**: r\n**Migration**: m\n\n"
    + "## RENAMED Requirements\n\n"
    + "- FROM: `### Requirement: Old Renamed`\n- TO: `### Requirement: New Renamed`\n"
    + "- FROM: ### Requirement: No Ticks Old\n"
    + "* FROM: `### Requirement: Star Old`\n"
    + "- from: `### Requirement: Lower Old`\n"
    + "- FROM: `### Requirement:  Spaced Old  `\n"
    + "- FROM: `Requirement: Short Old`\n\n"
    + req("Inside Renamed Section")
)

TASK_LINES_TASKS = """## 1. InHeading group

### InHeading sub

- [ ] 1.1 do InCheckbox now
  continuation mentions InContinuation
InPlainLine appears here
- [x] 1.2 InDoneBox
- plain bullet InPlainBullet
- [ ] 1.3 casemix lower
- [ ] 1.4 Partial Name
* [ ] 1.5 StarBox
+ [ ] 1.6 PlusBox
- [~] 1.7 TildeBox
- [ ]1.8 NoSpaceBox
1. [ ] NumberedBox
-\t[ ] TabBullet
    - [ ] 1.9 IndentedBox
- [X] 1.10 UpperXBox
- [ ] 1.11 new renamed
- [link text](url) LinkReq
- [a] LetterReq
- [ ]
- [] EmptyBracketReq
"""

DESIGN_TOPICS = """## Decisions

### 1. pqrs alpha
### 1.2.3 pqrs bravo
### 1.2. pqrs charlie
### 1 pqrs delta
###   7 pqrs echo
### 1 - pqrs foxtrot
### 1 2 3 pqrs golf
### 1) pqrs hotel
### 1: pqrs india
### 1.pqrs juliet
### 12abc pqrs kilo
### 3.x pqrs lima
### (1) pqrs mike
### 1、pqrs november
### ５ pqrs oscar
### a
### x y z
### 9
### the parser
### widget parser
### widget_gadget
### 版本升 widget
### 解析器設計
### alpha1 alpha2 missing
### alpha1 alpha2 alpha3 missing1 missing2
### alpha1 alpha2 alpha3 alpha4 missing1 missing2 missing3
###
#### Not a topic
##Not a topic
###Not a topic
  ### Indented Topic Missing
"""

TASKS_FOR_TOPICS = """## Tasks

- [ ] 1.1 cover pqrs things
- [ ] 1.2 build the parser widget and gadget
- [ ] 1.3 版本 checks
alpha1 alpha2 alpha3 alpha4 in a plain line
"""

AMBIGUITY_SPEC = """## ADDED Requirements

### Requirement: R1 abstract

The system should do.

#### Scenario: r1s

- **WHEN** b

### Requirement: R2 none

SHALL may.

### Requirement: R3 abstract

#### Scenario: r3s

- **WHEN** b

## MODIFIED Requirements

### Requirement: M1 none

SHALL.

## REMOVED Requirements

### Requirement: X1 none

**Reason**: might go

#### Scenario: removed abstract

- **WHEN** gone

## RENAMED Requirements

- FROM: `### Requirement: Old Name`
- TO: `### Requirement: New Name`

### Requirement: Inside renamed

text
"""

AMBIGUITY_WEAK = """## Purpose

TBD later.

## ADDED Requirements

### Requirement: A1 none

text

# Heading should skip

## Heading may skip

### Requirement: A2 should heading

See https://example.com/should/may for more.
See http://x.org/might and www.consider.com here.
Inline `should` code.
Plain mayhem word.
```
fenced should line
```
Question ??? here.
TODO and TKTK.
  # indented heading might
\t# tab heading possibly
"""


def concrete_spec() -> str:
    """One scenario per concrete-data case (SPEC section 4 plus boundaries)."""
    rows = [
        ("given", "- **GIVEN** a"),
        ("example", "##### Example: e"),
        ("table3", "| a | b |"),
        ("table2", "| a b |"),
        ("when-digit", "- **WHEN** 3 things"),
        ("when-glued", "- **WHEN**3"),
        ("then-quote", '- **THEN** "c"'),
        ("and-tick", "- **AND** `c`"),
        ("indented-when", "    - **WHEN** 3 things"),
        ("when-titlecase", "- **When** 3 things"),
        ("star-when", "* **WHEN** 3 things"),
        ("bare-given", "**GIVEN** x"),
        ("given-colon", "- **GIVEN:** x"),
        ("example-lower", "##### example: e"),
        ("pipes-inline", "a | b | c | d"),
        ("prose", 'The value is 3 and "q"'),
        ("fullwidth-digit", "- **THEN** 顯示３個"),
        ("fenced-given", "```\n- **GIVEN** a\n```"),
        ("fenced-tilde-when", "~~~\n- **WHEN** 3 things\n~~~"),
        ("then-plain", "- **THEN** ok"),
        ("h5-other-then-given", "##### Note\n- **GIVEN** a"),
        ("h4-other-then-given", "#### Note\n- **GIVEN** a"),
        ("h2-inside-then-given", "## Interlude\n- **GIVEN** a"),
        ("h1-inside-then-given", "# Title\n- **GIVEN** a"),
        ("h4-nospace-then-given", "####Note\n- **GIVEN** a"),
        ("h6-then-given", "###### Deep\n- **GIVEN** a"),
        ("h3-plain-then-given", "### Plain\n- **GIVEN** a"),
        ("crlf-given", "- **GIVEN** a\r"),
    ]
    out = "## ADDED Requirements\n\n### Requirement: Concrete\n\nSHALL.\n\n"
    for name, body in rows:
        out += f"#### Scenario: {name}\n\n{body}\n\n"
    out += "### Requirement: After\n\nSHALL.\n\n#### Scenario: after-req\n\n- **WHEN** w\n\n"
    out += "## MODIFIED Requirements\n\n#### Scenario: after-section\n\n- **WHEN** w\n"
    return out


DELTA_MULTI = {
    "v1-two-dups": "## ADDED Requirements\n\n" + req("A") + req("A") + req("B") + req("B"),
    "v2-dups-two-sections": "## ADDED Requirements\n\n" + req("A") + req("A")
    + "## MODIFIED Requirements\n\n" + req("B") + req("B"),
    "v3-three-sections": "## ADDED Requirements\n\n" + req("C") + "## MODIFIED Requirements\n\n" + req("C")
    + "## REMOVED Requirements\n\n### Requirement: C\n\n**Reason**: r\n\n",
    "v4-dup-and-cross": "## ADDED Requirements\n\n" + req("A") + req("A") + "## MODIFIED Requirements\n\n" + req("A"),
    "v5-repeated-header": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n### Requirement: First Block\n\nx\n\n"
    + "## ADDED Requirements\n\n### Requirement: Second Block\n\nx\n\n",
    "v6-repeated-dup": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("X") + "## ADDED Requirements\n\n" + req("X"),
    "v7-header-variants": "## Purpose\n\nP.\n\n## ADDED Requirements  \n\n" + req("Q") + req("Q")
    + "## added requirements\n\n" + req("R") + req("R") + "  ## MODIFIED Requirements\n\n" + req("Q"),
    "v8-renamed-dups": "## Purpose\n\nP.\n\n## RENAMED Requirements\n\n"
    + "- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n"
    + "- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n\n"
    + "## ADDED Requirements\n\n" + req("New"),
    "v9-purpose-and-dup": "## Purpose\n\n## ADDED Requirements\n\n" + req("D") + req("D"),
    "v11-renamed-cross": "## Purpose\n\nP.\n\n## RENAMED Requirements\n\n"
    + "- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n\n"
    + "## MODIFIED Requirements\n\n" + req("Old")
    + "## REMOVED Requirements\n\n### Requirement: New\n\n**Reason**: r\n\n",
    "v13-triple-and-removed-dup": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("T") + req("T") + req("T")
    + "## REMOVED Requirements\n\n### Requirement: U\n\n**Reason**: r\n\n### Requirement: U\n\n**Reason**: r\n\n",
    "f1-fenced-dup": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("A")
    + "```\n### Requirement: A\n```\n\n",
    "f2-fenced-section": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n```\n## MODIFIED Requirements\n```\n\n"
    + "### Requirement: B\n\nx\n",
    "f3-fenced-scenario": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n### Requirement: C\n\nx\n\n"
    + "```\n#### Scenario: fenced\n- **WHEN** w\n```\n",
    "f4-fenced-removed": "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("E")
    + "~~~\n## REMOVED Requirements\n~~~\n\n" + req("E"),
}

GAP_PURPOSE = {
    "p1-trailing": "## Purpose  \n\nP.\n\n## ADDED Requirements\n\n" + req("A"),
    "p2-leading": "  ## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("A"),
    "p3-h3-only": "## Purpose\n\n### Sub\n\n## ADDED Requirements\n\n" + req("A"),
    "p4-h1-after": "## Purpose\n\n# Title\n\nText.\n\n## ADDED Requirements\n\n" + req("A"),
    "p5-lowercase": "## purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("A"),
    "p6-suffix": "## Purpose of this\n\nP.\n\n## ADDED Requirements\n\n" + req("A"),
    "p7-fenced": "```\n## Purpose\n\nP.\n```\n\n## ADDED Requirements\n\n" + req("A"),
    "p8-two-purposes": "## Purpose\n\n## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("A"),
    "p9-todo-lower": "## Purpose\n\nwork todo\n\n## ADDED Requirements\n\n" + req("A"),
    "p10-tbd-word": "## Purpose\n\nTBDX and XTODO\n\n## ADDED Requirements\n\n" + req("A"),
    "p11-renamed-plus-added": "## RENAMED Requirements\n\n- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n\n"
    + "## ADDED Requirements\n\n" + req("A"),
    "p12-removed-empty": "## REMOVED Requirements\n\n",
    "p13-crlf": "## Purpose\r\n\r\n## ADDED Requirements\r\n\r\n" + req("A").replace("\n", "\r\n"),
}

TOKENS8 = "alpha bravo charlie delta echoo foxtrot golf1 hotel"

LOC_EN = (
    "This change introduces a completely new widget pipeline so that operators "
    "can observe throughput and latency in real time."
)
B79 = " ".join(["xxxx"] * 19) + " xxx"
H100 = "y" * 100


def loc_cases() -> dict:
    """Localization detection, one change per case (proposal.md only)."""
    a80 = " ".join(["xxxx"] * 20)
    a72 = " ".join(["xxxx"] * 18)
    return {
        "n79": B79,
        "n80": a80,
        "n80-8cjk": a80 + " " + "漢" * 8,
        "n80-9cjk": a80 + " " + "漢" * 9,
        "n72-8cjk": a72 + " " + "漢" * 8,
        "kana": a80 + " " + "カ" * 9,
        "hangul": a80 + " " + "한" * 9,
        "compat": a80 + " " + "\uf900" * 9,
        "han-8c48": a80 + " " + "\u8c48" * 9,
        "latin1": B79 + " é",
        "digits": B79 + " 1234567890",
        "fence-info": B79 + "\n```rust\n" + H100 + "\n```\n",
        "fence-mixed": B79 + "\n```\n" + H100 + "\n~~~\n" + H100 + "\n",
        "fence-open": B79 + "\n```\n" + H100 + "\n",
        "fence-2bt": B79 + "\n``\n" + H100 + "\n``\n",
        "fence-inline": B79 + "\ntext ```\n" + H100 + "\n",
        "inline": B79 + " `" + H100 + "`",
        "inline-double": B79 + " ``" + H100 + "``",
        "inline-unclosed": B79 + " `" + H100,
        "https": B79 + " https://" + H100 + ".com/" + H100,
        "www-mid": B79 + " a" + "www." + H100,
        "url-then": B79 + " https://x.com " + H100,
        "url-paren": B79 + " https://x.com)" + H100,
        "url-comma": B79 + " https://x.com," + H100,
        "url-nbsp": " ".join(["xxxx"] * 19) + " xx i https://x.com " + H100,
        "url-upper": B79 + " HTTPS://" + H100,
        "comment": B79 + "\n<!-- " + H100 + " -->\n",
        "crlf": B79 + " x\r\n",
        "empty": "",
    }


def localization_files() -> dict:
    return {ch(f"loc-{case}", "proposal.md"): text + ("\n" if text else "") for case, text in loc_cases().items()}


SCENARIOS = [
    {
        "name": "presence-layouts",
        "description": "Only a regular specs/<dir>/spec.md one level deep makes specs present; "
        "nested files are ignored by every rule.",
        "files": {
            ch("empty-change", ".keep"): "",
            ch("nested", "proposal.md"): "## Why\n\nx\n",
            ch("nested", "specs/a/b/spec.md"): "## ADDED Requirements\n\n" + req("Nested should", scenario=SC_ABSTRACT),
            ch("flat", "proposal.md"): "## Why\n\nx\n",
            ch("flat", "specs/x.md"): "## ADDED Requirements\n\n" + req("Flat"),
            ch("other-md", "proposal.md"): "## Why\n\nx\n",
            ch("other-md", "specs/x/other.md"): "## ADDED Requirements\n\n" + req("Other"),
            ch("dir-named-spec", "proposal.md"): "## Why\n\nx\n",
            ch("dir-named-spec", "specs/x/spec.md/.keep"): "",
            ch("empty-spec", "proposal.md"): "## Why\n\nx\n",
            ch("empty-spec", "specs/x/spec.md"): "",
            ch("mixed", "proposal.md"): "## Why\n\nx\n\n## Capabilities\n\n- `top`: x\n- `deep/leaf`: x\n",
            ch("mixed", "tasks.md"): "- [ ] 1.1 Top\n",
            ch("mixed", "specs/top/spec.md"): "## ADDED Requirements\n\n" + req("Top"),
            ch("mixed", "specs/deep/leaf/spec.md"): "## ADDED Requirements\n\n" + req("Deep should", scenario=SC_ABSTRACT),
            ch("specs-only", "specs/x/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("X"),
        },
        "runs": both("empty-change", "nested", "flat", "other-md", "dir-named-spec", "empty-spec", "mixed", "specs-only"),
    },
    {
        "name": "coverage-capabilities",
        "description": "Capability extraction: first backtick token per section line, spaced/empty "
        "tokens rejected, section boundaries, and the exact spec-dir-name missing test.",
        "files": {
            ch("c", "proposal.md"): CAPABILITIES_PROPOSAL,
            ch("c", "tasks.md"): "- [ ] 1.1 Req-A\n",
            ch("c", "specs/cap-ok/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("Req-A"),
            ch("c", "specs/nested/cap/spec.md"): "## ADDED Requirements\n\n" + req("Req-N"),
            ch("headings", "proposal.md"): "## Why\n\n- `before`: x\n\n"
            "## capabilities\n\n- `lower`: x\n\n"
            "# Capabilities\n\n- `h1`: x\n\n"
            "  ## Capabilities\n\n- `indented-open`: x\n\n"
            "## Impact\n\n- `impact`: x\n\n"
            "### Modified Capabilities\n\n- `modified-open`: x\n\n"
            "## Next\n\n"
            "### New Capabilities\n\n- `new-open`: x\n",
            ch("headings", "design.md"): "## Context\n\nx\n",
            ch("case", "proposal.md"): "## Capabilities\n\n- `x`: only `X` exists\n- `X`: exact\n",
            ch("case", "specs/X/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("Req-X"),
        },
        "runs": both("c", "headings", "case"),
    },
    {
        "name": "coverage-tasks",
        "description": "covMissingTask: task lines only, REMOVED skipped, RENAMED FROM checked, "
        "duplicates reported per occurrence.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n",
            ch("c", "specs/cap/spec.md"): TASK_LINES_SPEC,
            ch("c", "tasks.md"): TASK_LINES_TASKS,
            ch("outside", "proposal.md"): "## Why\n\nx\n",
            ch("outside", "tasks.md"): "- [ ] 1.1 nothing relevant\n",
            ch("outside", "specs/cap/spec.md"): "### Requirement: Before Any Section\n\n"
            + SC
            + "\n## Requirements\n\n"
            + req("Under Plain Requirements")
            + "## Notes\n\n"
            + req("Under Notes")
            + "## ADDED Requirements\n\n"
            + "  ### Requirement: Indented Header\n\n"
            + SC
            + "\n### Requirement:\n\n"
            + SC
            + "\n#### Requirement: Level Four\n\n"
            + "## RENAMED Requirements\n\n"
            + "- FROM: Bare Old\n- TO: Bare New\n"
            + "- FROM: `### Requirement: Ticked Old`\n- TO: `### Requirement: Ticked New`\n"
            + "  - FROM: `### Requirement: Indented Old`\n"
            + "- FROM:`### Requirement: Glued Old`\n",
        },
        "runs": both("c", "outside"),
    },
    {
        "name": "coverage-order-and-gating",
        "description": "Coverage groups findings per kind across spec files; the dimension runs "
        "with any two of the four artifacts.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n\n## Capabilities\n\n- `zz-missing`: x\n- `aa-missing`: x\n",
            ch("c", "tasks.md"): "- [ ] 1.1 nothing\n",
            ch("c", "specs/bbb/spec.md"): "## ADDED Requirements\n\n" + req("Dup") + req("Dup") + req("A only"),
            ch("c", "specs/ccc/spec.md"): "## Purpose\n\n\n## ADDED Requirements\n\n"
            + req("B1")
            + "## REMOVED Requirements\n\n### Requirement: B1\n\n**Reason**: r\n",
            ch("design-tasks", "design.md"): "## Context\n\nx\n",
            ch("design-tasks", "tasks.md"): "- [ ] 1.1 x\n",
            ch("proposal-design", "proposal.md"): "## Capabilities\n\n- `gone`: x\n",
            ch("proposal-design", "design.md"): "## Context\n\nx\n",
            ch("specs-design", "design.md"): "## Context\n\nx\n",
            ch("specs-design", "specs/cap/spec.md"): "## Purpose\n\nTODO\n\n## ADDED Requirements\n\n" + req("C"),
            ch("proposal-only", "proposal.md"): "## Capabilities\n\n- `gone`: x\n",
            ch("specs-tasks", "tasks.md"): "- [ ] 1.1 nothing\n",
            ch("specs-tasks", "specs/cap/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("Needs Task"),
        },
        "runs": both("c", "design-tasks", "proposal-design", "specs-design", "proposal-only", "specs-tasks"),
    },
    {
        "name": "delta-validation",
        "description": "covDeltaValidation errors reported by analyze, including the 3.0.0 Purpose "
        "checks, and the malformed shapes it does not report.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n",
            ch("c", "tasks.md"): "- [ ] 1.1 A B C D E F G H I J K L M N O P\n",
            ch("c", "specs/c01-rs/spec.md"): "## Purpose\n\n## ADDED Requirements\n\n" + req("A"),
            ch("c", "specs/c02-ub/spec.md"): "## Purpose\n\nTODO: fill\n\n## ADDED Requirements\n\n" + req("B"),
            ch("c", "specs/c03-ag/spec.md"): "## Purpose\n\nP.\n\n## Requirements\n\n" + req("C"),
            ch("c", "specs/c04-od/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n### Requirement:\n\nx\n\n" + SC,
            ch("c", "specs/c05-xi/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n### Req: D\n\nx\n\n" + SC,
            ch("c", "specs/c06-zn/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("E") + req("E"),
            ch("c", "specs/c07-dy/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n"
            + req("F")
            + "## REMOVED Requirements\n\n### Requirement: F\n\n**Reason**: r\n",
            ch("c", "specs/c08-fh/spec.md"): "",
            ch("c", "specs/c09-rm/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n",
            ch("c", "specs/c10-fc/spec.md"): "## Purpose\n\ntbd\n\n## ADDED Requirements\n\n" + req("G"),
            ch("c", "specs/c11-ro/spec.md"): "## Purpose\n\nP.\n\n## RENAMED Requirements\n\n- FROM: H\n",
            ch("c", "specs/c12-yu/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n#### Requirement: I\n\nx\n",
            ch("c", "specs/c13-gb/spec.md"): "## Purpose\n\n   \n\t\n## ADDED Requirements\n\n" + req("J"),
            ch("c", "specs/c14-ia/spec.md"): "## Purpose\n\n### Sub heading\n\nText.\n\n## ADDED Requirements\n\n" + req("K"),
            ch("c", "specs/c15-xu/spec.md"): "## Purpose\n\n<!-- comment only -->\n\n## ADDED Requirements\n\n" + req("L"),
            ch("c", "specs/c16-so/spec.md"): "## Purpose\n\nStill TBD here.\n\n## ADDED Requirements\n\n"
            + req("M")
            + req("M")
            + "## MODIFIED Requirements\n\n"
            + req("M")
            + req("N")
            + "## ADDED Requirements\n\n"
            + req("N"),
            ch("c", "specs/c17-nw/spec.md"): "## ADDED Requirements\n\n"
            + req("O")
            + "## Purpose\n\n## MODIFIED Requirements\n\n"
            + req("O"),
            ch("c", "specs/c18-lf/spec.md"): "  ## Purpose  \n\nIndented.\n\n## ADDED Requirements\n\n" + req("P") + req("  P  "),
        },
        "runs": both("c"),
    },
    {
        "name": "delta-validation-multi",
        "description": "How many covDeltaValidation errors one file yields, and repeated section headers.",
        "files": {
            **{ch(name, "proposal.md"): "## Why\n\nx\n" for name in DELTA_MULTI},
            **{ch(name, "tasks.md"): "- [ ] 1.1 nothing\n" for name in DELTA_MULTI},
            **{ch(name, "specs/cap/spec.md"): text for name, text in DELTA_MULTI.items()},
        },
        "runs": [json_run(name) for name in DELTA_MULTI],
    },
    {
        "name": "consistency-design-topics",
        "description": "conDesignNotInTasks: numbering-prefix strip, significant tokens, 60% rule, "
        "substring match against the whole tasks.md.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n",
            ch("c", "design.md"): DESIGN_TOPICS,
            ch("c", "tasks.md"): TASKS_FOR_TOPICS,
        },
        "runs": both("c"),
    },
    {
        "name": "consistency-gating",
        "description": "Consistency runs with design, or with proposal and tasks; each rule has "
        "its own preconditions.",
        "files": {
            ch("design-only", "design.md"): "## Decisions\n\n### Unreferenced Topic\n",
            ch("proposal-tasks", "proposal.md"): "## Why\n\nx\n",
            ch("proposal-tasks", "tasks.md"): "- [ ] 1.1 x\n",
            ch("design-goals", "design.md"): f"**Goals:**\n- {TOKENS8}\n\n**Non-Goals:**\n- {TOKENS8}\n\n### Topic Without Tasks\n",
            ch("tasks-only", "tasks.md"): "- [ ] 1.1 x\n",
        },
        "runs": both("design-only", "proposal-tasks", "design-goals", "tasks-only"),
    },
    {
        "name": "consistency-goals-overlap",
        "description": "conGoalsNonGoalsOverlap: >=8 shared tokens and >=40% of the smaller item; "
        "marker, bullet, fence and heading handling.",
        "files": {
            ch("g8", "design.md"): f"**Goals:**\n- {TOKENS8}\n\n**Non-Goals:**\n- {TOKENS8}\n",
            ch("g7", "design.md"): "**Goals:**\n- alpha bravo charlie delta echoo foxtrot golf1\n\n"
            "**Non-Goals:**\n- alpha bravo charlie delta echoo foxtrot golf1\n",
            ch("g20", "design.md"): "**Goals:**\n- " + TOKENS8 + " aaa1 aaa2 aaa3 aaa4 aaa5 aaa6 aaa7 aaa8 aaa9 aa10 aa11 aa12\n\n"
            "**Non-Goals:**\n- " + TOKENS8 + " bbb1 bbb2 bbb3 bbb4 bbb5 bbb6 bbb7 bbb8 bbb9 bb10 bb11 bb12\n",
            ch("g21", "design.md"): "**Goals:**\n- " + TOKENS8 + " aaa1 aaa2 aaa3 aaa4 aaa5 aaa6 aaa7 aaa8 aaa9 aa10 aa11 aa12 aa13\n\n"
            "**Non-Goals:**\n- " + TOKENS8 + " bbb1 bbb2 bbb3 bbb4 bbb5 bbb6 bbb7 bbb8 bbb9 bb10 bb11 bb12 bb13\n",
            ch("shapes", "design.md"): "## Goals / Non-Goals\n\n**Goals:**\n- alpha bravo charlie delta\n  echoo foxtrot golf1 hotel\n"
            "* second goal item with words\n+ " + TOKENS8 + " india\n\n**Non-Goals:**\n- 非 " + TOKENS8 + "\n- "
            + TOKENS8 + " india\n1. " + TOKENS8 + "\n\n## Decisions\n",
            ch("nongoals-first", "design.md"): f"**Non-Goals:**\n- {TOKENS8}\n\n## Other\n\n  **Goals:**\n- {TOKENS8}\n",
            ch("fenced", "design.md"): f"**Goals:**\n- {TOKENS8}\n\n```\n**Non-Goals:**\n- {TOKENS8}\n```\n",
            ch("markers", "design.md"): f"## Goals\n\n- {TOKENS8}\n\n**Goals**:\n- {TOKENS8}\n\n**Non-Goals:**\n{TOKENS8}\n",
            ch("cjk", "design.md"): "**Goals:**\n- 支援離線編輯模式讓使用者可以\n\n**Non-Goals:**\n- 支援離線編輯模式讓使用者可以\n",
            ch("in-proposal", "proposal.md"): f"**Goals:**\n- {TOKENS8}\n\n**Non-Goals:**\n- {TOKENS8}\n",
            ch("in-proposal", "tasks.md"): "- [ ] 1.1 x\n",
        },
        "runs": [
            json_run(c)
            for c in ("g8", "g7", "g20", "g21", "shapes", "nongoals-first", "fenced", "markers", "cjk", "in-proposal")
        ]
        + [human_run("shapes")],
    },
    {
        "name": "ambiguity",
        "description": "Ambiguity per file: no-scenario, abstract-scenario, weak-language groups; "
        "REMOVED requirements skipped; headings skipped by weak language.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n",
            ch("c", "specs/bbb/spec.md"): AMBIGUITY_SPEC,
            ch("c", "specs/ccc/spec.md"): AMBIGUITY_WEAK,
            ch("concrete", "proposal.md"): "## Why\n\nx\n",
            ch("concrete", "specs/cap/spec.md"): concrete_spec(),
            ch("req-blocks", "proposal.md"): "## Why\n\nx\n",
            ch("req-blocks", "specs/cap/spec.md"): "## ADDED Requirements\n\n"
            "### Requirement: After H3\n\nx\n\n### Interface\n\ny\n\n### Scenarios\n\n#### Scenario: a\n\n- **GIVEN** g\n\n"
            "### Requirement: After H1\n\nx\n\n# Title\n\n#### Scenario: b\n\n- **GIVEN** g\n\n"
            "### Requirement: Before Next\n\nx\n\n"
            "### Requirement: Next\n\n#### Scenario: c\n\n- **GIVEN** g\n\n"
            "### Requirement: After H2\n\nx\n\n## Notes\n\n#### Scenario: d\n\n- **GIVEN** g\n\n"
            "## MODIFIED Requirements\n\n### Requirement: Indented Next\n\n  ### Requirement: Indented Req\n\n#### Scenario: e\n\n- **GIVEN** g\n",
            ch("renamed-reqs", "proposal.md"): "## Why\n\nx\n",
            ch("renamed-reqs", "tasks.md"): "- [ ] 1.1 nothing\n",
            ch("renamed-reqs", "specs/cap/spec.md"): "## Purpose\n\nP.\n\n## RENAMED Requirements\n\n"
            "- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n\n"
            "### Requirement: Inside Renamed\n\nThe system SHALL.\n\n"
            "## REMOVED Requirements\n\n### Requirement: Inside Removed\n\n**Reason**: r\n\n"
            "## Notes\n\n### Requirement: Inside Notes\n\nx\n\n"
            "## ADDED Requirements\n\n### Requirement: Scenario Under Other Heading\n\n"
            "#### Note\n\n#### Scenario: late\n\n- **GIVEN** g\n",
        },
        "runs": both("c", "concrete") + [json_run("renamed-reqs"), json_run("req-blocks")],
    },
    {
        "name": "gaps",
        "description": "gapNewCapabilityNoPurpose conditions and the three Gaps emission passes.",
        "files": {
            ch("c", "proposal.md"): "## Why\n\nx\n",
            ch("c", "tasks.md"): "- [ ] 1.1 A B C D E F G H I Z\n",
            main_spec("g04-cj"): "## Purpose\n\nP.\n\n## Requirements\n\n### Requirement: Z\n\nx\n",
            main_spec("g05-tg"): "## Purpose\n\nP.\n\n## Requirements\n\n### Requirement: Z\n\nx\n",
            ch("c", "specs/g01-qy/spec.md"): "## ADDED Requirements\n\n" + req("A"),
            ch("c", "specs/g02-ug/spec.md"): "## MODIFIED Requirements\n\n" + req("B"),
            ch("c", "specs/g03-kp/spec.md"): "## REMOVED Requirements\n\n### Requirement: C\n\n**Reason**: r\n",
            ch("c", "specs/g04-cj/spec.md"): "## ADDED Requirements\n\n" + req("D"),
            ch("c", "specs/g05-tg/spec.md"): "## MODIFIED Requirements\n\n" + req("E") + req("Z"),
            ch("c", "specs/g06-yd/spec.md"): "### Purpose\n\nP.\n\n## ADDED Requirements\n\n" + req("F"),
            ch("c", "specs/g07-be/spec.md"): "## ADDED Requirements\n\n" + req("G") + "## Purpose\n\nP.\n",
            ch("c", "specs/g08-fm/spec.md"): "## Purpose:\n\nP.\n\n## ADDED Requirements\n\n" + req("H"),
            ch("c", "specs/g09-xz/spec.md"): "## RENAMED Requirements\n\n- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n",
            ch("c", "specs/g10-cp/spec.md"): "## Notes\n\n" + req("Under Notes"),
            ch("c", "specs/g11-xq/spec.md"): "## Purpose\n\n  \n\n## ADDED Requirements\n\n" + req("I"),
        },
        "runs": both("c"),
    },
    {
        "name": "gaps-purpose-headings",
        "description": "Which headings count as a (non-empty) ## Purpose, for the gap and the validation error.",
        "files": {
            **{ch(name, "proposal.md"): "## Why\n\nx\n" for name in GAP_PURPOSE},
            **{ch(name, "specs/cap/spec.md"): text for name, text in GAP_PURPOSE.items()},
        },
        "runs": [json_run(name) for name in GAP_PURPOSE],
    },
    {
        "name": "localization-gating-and-files",
        "description": "Localization runs only for locale tw/cn/ja with proposal, design or tasks "
        "present; checks those three files in order; its findings come first.",
        "locale": "tw",
        "files": {
            ch("c", "proposal.md"): f"## Why\n\n{LOC_EN}\n\n## Capabilities\n\n- `missing-cap`: x\n",
            ch("c", "design.md"): f"## Context\n\n{LOC_EN}\n",
            ch("c", "tasks.md"): f"- [ ] 1.1 {LOC_EN}\n",
            ch("c", "testplan.md"): f"## Test plan\n\n{LOC_EN}\n",
            ch("c", "README.md"): f"{LOC_EN}\n",
            ch("c", "specs/cap/spec.md"): f"## ADDED Requirements\n\n### Requirement: Alpha\n\n{LOC_EN}\n",
            ch("specs-only", "specs/cap/spec.md"): f"## Purpose\n\n{LOC_EN}\n\n## ADDED Requirements\n\n" + req("Alpha"),
            ch("cjk-proposal", "proposal.md"): "## Why\n\n這是一個完整的中文提案內容說明文字\n",
            ch("cjk-proposal", "design.md"): f"## Context\n\n{LOC_EN}\n",
        },
        "runs": both("c", "specs-only", "cjk-proposal"),
    },
    {
        "name": "localization-detection",
        "description": "is_wrong_language: 80-letter floor, 10% CJK ratio, character classes, "
        "fence/inline-code/URL removal.",
        "locale": "ja",
        "files": localization_files(),
        "runs": [json_run(f"loc-{case}") for case in loc_cases()],
    },
    {
        "name": "localization-locales",
        "description": "Only the exact locale values tw, cn and ja enable the dimension.",
        "locale_variants": ["cn", "ja", "zh", "zh-TW", "TW", "en", '"tw"  # quoted'],
        "files": {
            ch("c", "proposal.md"): f"## Why\n\n{LOC_EN}\n",
        },
        "runs": [json_run("c")],
    },
    {
        "name": "human-output",
        "description": "Human report with every severity, plain and on a terminal (colours).",
        "locale": "cn",
        "files": {
            ch("c", "proposal.md"): f"## Why\n\n{LOC_EN}\n\n## Capabilities\n\n- `nope`: x\n",
            ch("c", "specs/cap/spec.md"): "## Purpose\n\nP.\n\n## ADDED Requirements\n\n### Requirement: Alpha\n\n"
            "The system should.\n\n#### Scenario: s\n\n- **GIVEN** b\n",
            ch("clean", "proposal.md"): "## Why\n\n中文內容說明\n",
        },
        "runs": both("c", "clean")
        + [
            {"args": ["analyze", "c"], "tty": True},
            {"args": ["analyze", "clean"], "tty": True},
            {"args": ["analyze", "c", "--no-color"], "tty": True},
            {"args": ["--no-color", "analyze", "clean"], "tty": True},
            {"args": ["analyze", "c", "--json"], "tty": True},
            {"args": ["analyze", "c", "--no-color"]},
        ],
    },
    {
        "name": "numeric-claims",
        "description": "conNumericClaimMismatch: extraction, drops, label matching and greedy pairing "
        "(the W10 p40-p59 probe cases, one change each).",
        "files": {
            ch(name, f"{kind}.md"): text
            for name, case in NUMERIC_CASES.items()
            for kind, text in case.items()
            if text is not None
        },
        "runs": [json_run(name) for name in NUMERIC_CASES] + [human_run("p44a")],
    },
    {
        "name": "change-resolution",
        "description": "Multiple active changes without a name, unknown and archived names.",
        "files": {
            ch("aa-first", "proposal.md"): "## Why\n\nx\n",
            ch("bb-second", "proposal.md"): "## Why\n\nx\n",
            f"{SPEC_DIR}/changes/archive/2026-01-01-old/proposal.md": "## Why\n\nx\n",
        },
        "runs": [
            {"args": ["analyze"]},
            {"args": ["analyze", "--json"]},
            {"args": ["analyze", "nope"]},
            {"args": ["analyze", "2026-01-01-old", "--json"]},
        ],
    },
]


def fail(message: str) -> NoReturn:
    print(f"[FAIL] {message}", file=sys.stderr)
    sys.exit(1)


def env() -> dict:
    e = dict(os.environ)
    for key in ("NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "OPENSPECTRA_IMPL"):
        e.pop(key, None)
    return e


def run_plain(args: list, cwd: Path) -> tuple:
    try:
        r = subprocess.run(args, cwd=cwd, env=env(), capture_output=True, timeout=60, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"無法執行 {args!r}：{error}")
    return r.returncode, r.stdout, r.stderr


def run_tty(args: list, cwd: Path) -> tuple:
    """stdout 接到 pseudo-terminal，stderr 仍用 pipe；PTY 的 \\r\\n 折回 \\n。"""
    master, slave = pty.openpty()
    try:
        proc = subprocess.Popen(args, cwd=cwd, env=env(), stdin=subprocess.DEVNULL, stdout=slave, stderr=subprocess.PIPE)
    except OSError as error:
        fail(f"無法執行 {args!r}：{error}")
    os.close(slave)
    chunks = []
    while True:
        try:
            data = os.read(master, 65536)
        except OSError:  # 子行程關閉 PTY 後 macOS 回 EIO
            break
        if not data:
            break
        chunks.append(data)
    os.close(master)
    stderr = proc.stderr.read()
    code = proc.wait(timeout=60)
    return code, b"".join(chunks).replace(b"\r\n", b"\n"), stderr


def write(root: Path, rel: str, content: str) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content.encode())


def change_names(files: dict) -> list:
    prefix = f"{SPEC_DIR}/changes/"
    names = set()
    for rel in files:
        if rel.startswith(prefix):
            name = rel[len(prefix):].split("/", 1)[0]
            if name != "archive":
                names.add(name)
    return sorted(names)


def tree(scenario: dict, locale) -> dict:
    files = dict(BASE_FILES)
    if locale is not None:
        files[".spectra.yaml"] = f"spec_dir: {SPEC_DIR}\nlocale: {locale}\n"
    for name in change_names(scenario["files"]):
        files[ch(name, ".openspec.yaml")] = OPENSPEC_YAML
    files.update(scenario["files"])
    return files


def check_readdir_order(root: Path, lists_changes: bool) -> None:
    """OpenSpectra 依名稱排序走訪；fixture 的 readdir 順序必須剛好是名稱排序，golden 才能比對。
    change 目錄的順序只在未指定 change 的錯誤訊息中可見，所以只在有那種執行時檢查。"""
    changes = root / SPEC_DIR / "changes"
    listing = [n for n in os.listdir(changes) if (changes / n).is_dir() and n != "archive"]
    if lists_changes and listing != sorted(listing):
        fail(f"{changes} 的 readdir 順序 {listing} 不是名稱排序；請改 fixture 的 change 名稱")
    for name in listing:
        specs = changes / name / "specs"
        if specs.is_dir():
            dirs = [n for n in os.listdir(specs) if (specs / n / "spec.md").is_file()]
            if dirs != sorted(dirs):
                fail(f"{specs} 的 readdir 順序 {dirs} 不是名稱排序；請改 fixture 的目錄名稱")


def canonical_json(raw: bytes, where: str) -> str:
    text = raw.decode()
    value = json.loads(text)
    if json.dumps(value, indent=2, ensure_ascii=False) + "\n" != text:
        fail(f"{where}：重新序列化無法還原 oracle 的 JSON bytes，正規化不可信")

    def sort_params(node):
        if isinstance(node, dict):
            return {
                k: (dict(sorted(v.items())) if k == "params" and isinstance(v, dict) else sort_params(v))
                for k, v in node.items()
            }
        if isinstance(node, list):
            return [sort_params(x) for x in node]
        return node

    return json.dumps(sort_params(value), indent=2, ensure_ascii=False) + "\n"


def run_project(binary: Path, scenario: dict, locale, work: Path, label: str) -> dict:
    root = work / label
    root.mkdir(parents=True)
    root = root.resolve()
    files = tree(scenario, locale)
    for rel, content in files.items():
        write(root, rel, content)
    lists_changes = any(all(a.startswith("-") or a == "analyze" for a in run["args"]) for run in scenario["runs"])
    check_readdir_order(root, lists_changes)
    runs = []
    for run in scenario["runs"]:
        tty = run.get("tty", False)
        runner = run_tty if tty else run_plain
        code, out, err = runner([str(binary), *run["args"]], root)
        is_json = "--json" in run["args"] and code == 0 and out.startswith(b"{")
        stdout = canonical_json(out, f"{label} {run['args']}") if is_json else out.decode()
        runs.append({"args": run["args"], "tty": tty, "expect": {"exit": code, "stdout": stdout, "stderr": err.decode()}})
    return {"locale": locale, "files": files, "runs": runs}


def capture_unchecked(binary: Path, work: Path) -> bytes:
    code, out, _ = run_plain([str(binary), "--version"], work)
    parts = out.decode().split()
    if code != 0 or len(parts) < 2 or parts[0] != "spectra":
        fail(f"無法解析參考執行檔版本：{out!r}")
    if parts[1] != EXPECTED_VERSION:
        fail(f"參考執行檔版本為 {parts[1]}，本腳本固定 {EXPECTED_VERSION}。")
    names = [sc["name"] for sc in SCENARIOS]
    if len(set(names)) != len(names):
        fail(f"情境名稱重複：{names}")
    scenarios = []
    for sc in SCENARIOS:
        variants = sc.get("locale_variants", [sc.get("locale")])
        projects = [run_project(binary, sc, loc, work, f"{sc['name']}-{i}") for i, loc in enumerate(variants)]
        scenarios.append({"name": sc["name"], "description": sc["description"], "projects": projects})
    golden = {"oracle_version": EXPECTED_VERSION, "scenarios": scenarios}
    return (json.dumps(golden, indent=2, ensure_ascii=False) + "\n").encode()


def capture(binary: Path, work: Path) -> bytes:
    try:
        return capture_unchecked(binary, work)
    except SystemExit:
        raise
    except Exception as error:  # 例如非 UTF-8 輸出
        fail(f"捕獲時發生未預期的例外：{error!r}；scratch 專案保留於 {work}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--spectra-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_BIN))
    parser.add_argument("--write", action="store_true", help="regenerate the golden, then verify")
    args = parser.parse_args()

    binary = Path(args.spectra_bin)
    if not os.access(binary, os.X_OK):
        fail(f"找不到可執行的參考執行檔：{binary}（以 --spectra-bin 或 SPECTRA_BIN 指定）")
    repo_root = Path(__file__).resolve().parent.parent
    golden_path = repo_root / GOLDEN_REL

    work = Path(tempfile.mkdtemp(prefix="capture-analyze-"))
    actual = capture(binary, work)
    if args.write:
        candidate = work / "candidate.json"
        candidate.write_bytes(actual)
        recheck_work = Path(tempfile.mkdtemp(prefix="capture-analyze-"))
        recheck = capture(binary, recheck_work)
        if recheck != actual:
            recheck_path = recheck_work / "actual.json"
            recheck_path.write_bytes(recheck)
            fail(f"兩次捕獲不一致，未覆寫 {GOLDEN_REL}。candidate：{candidate}；重新捕獲：{recheck_path}")
        golden_path.write_bytes(actual)
        print(f"[OK] 已寫入 {GOLDEN_REL}")
        shutil.rmtree(recheck_work)

    if not golden_path.exists():
        fail(f"{GOLDEN_REL} 不存在；以 --write 產生。scratch 專案保留於 {work}")
    if actual != golden_path.read_bytes():
        drift_path = work / "actual.json"
        drift_path.write_bytes(actual)
        fail(f"oracle 捕獲與 {GOLDEN_REL} 不一致。實際輸出：{drift_path}；scratch 專案保留於 {work}")
    data = json.loads(actual)
    count = sum(len(p["runs"]) for sc in data["scenarios"] for p in sc["projects"])
    shutil.rmtree(work)
    print(f"[OK] {len(SCENARIOS)} 個情境、{count} 次執行與 oracle {EXPECTED_VERSION} 一致")


if __name__ == "__main__":
    main()
