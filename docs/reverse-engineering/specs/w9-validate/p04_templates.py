#!/usr/bin/env python3
"""W9 p04：把 OpenSpec / OpenSpectra 的 finding 訊息歸成規則類別，逐 item 比對類別層級與判定。"""
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

R = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re/p02")

RULES = [
    ("scenario_loss", r"omits scenario\(s\) the current spec still has"),
    ("no_deltas", r"^Change must (have|contain) at least one delta"),
    ("no_delta_sections", r"^No delta sections found"),
    ("stray_header_info", r"is not a \"### Requirement:\" header and is ignored"),
    ("archive_refuse", r"^Archive would refuse this delta|cannot MODIFY requirement"),
    ("trace_footer", r"unrecognized `<!-- @trace` footer"),
    ("added_no_scenario", r"^(ADDED|MODIFIED) \".*\" must include at least one scenario|must have at least one `#### Scenario:`"),
    ("shall_must", r"SHALL or MUST"),
    ("missing_text", r"missing requirement text"),
    ("purpose_placeholder", r"placeholder"),
    ("purpose_brief", r"Purpose section is too brief"),
    ("purpose_missing", r"Spec must have a Purpose section|non-empty ## Purpose|Purpose section cannot be empty"),
    ("requirements_missing", r"Spec must have a Requirements section"),
    ("no_requirements", r"Spec must have at least one requirement|at least one requirement under ## Requirements"),
    ("req_no_scenario_zod", r"^Requirement must have at least one scenario$"),
    ("req_no_scenario_rule", r"^Requirement must have at least one scenario\. Scenarios must use"),
    ("req_no_scenario_os", r"must include a scenario$"),
    ("req_too_long", r"Requirement text is very long"),
    ("main_structure", r"^Main spec contains|delta header|structurally"),
    ("task_numbering", r"^Task (ID )?\""),
    ("task_checkbox", r"checkbox"),
    ("orphan_req", r"which is not a delta section"),
]


def classify(msg):
    for name, pat in RULES:
        if re.search(pat, msg):
            return name
    return "OTHER:" + msg[:90].replace("\n", " ")


def items(doc):
    return {it["id"]: it for it in doc.get("items", [])}


verdict = Counter()
pairs = Counter()
examples = defaultdict(list)
for proj in ["yibi-mvp", "nextrek-cli", "yibi-stack"]:
    for scope in ["changes", "specs"]:
        op = items(json.loads((R / f"{proj}.{scope}.openspec.json").read_text()))
        osx = items(json.loads((R / f"{proj}.{scope}.os.json").read_text()))
        oracle = {(it.get("change") or it.get("spec")): it for it in json.loads((R / f"{proj}.{scope}.oracle.json").read_text())}
        for i in sorted(set(op) | set(osx)):
            a = Counter((classify(x["message"]), x["level"]) for x in op.get(i, {}).get("issues", []))
            b = Counter((classify(x["message"]), x["level"]) for x in osx.get(i, {}).get("issues", []))
            verdict[(scope, oracle.get(i, {}).get("valid"), op.get(i, {}).get("valid"), osx.get(i, {}).get("valid"))] += 1
            if op.get(i, {}).get("valid") != osx.get(i, {}).get("valid"):
                examples["VERDICT"].append(f"{proj}/{scope}/{i}: openspec={op.get(i, {}).get('valid')} openspectra={osx.get(i, {}).get('valid')} "
                                           f"op={dict(a)} os={dict(b)}")
            for k in set(a) | set(b):
                if a[k] != b[k]:
                    pairs[(scope, k[0], k[1], a[k], b[k]) if False else (scope, k[0], k[1], "openspec" if a[k] > b[k] else "openspectra")] += abs(a[k] - b[k])
                    if len(examples[(scope, k)]) < 3:
                        examples[(scope, k)].append(f"{proj}/{i} op={a[k]} os={b[k]}")

print("## verdict combos (scope, oracle, openspec, openspectra) -> count")
for k, v in sorted(verdict.items(), key=str):
    print(f"  {k}: {v}")
print("\n## rule-class count differences (scope, class, level, side-with-more) -> surplus")
for k, v in sorted(pairs.items(), key=str):
    print(f"  {k}: {v}   e.g. {examples[(k[0], (k[1], k[2]))][:2]}")
print("\n## verdict differences")
for e in examples["VERDICT"]:
    print("  " + e)
