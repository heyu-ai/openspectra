#!/usr/bin/env python3
"""W9 p13：SPEC.md 內幾個計數與 key 順序的查證。"""
import json
from pathlib import Path

R = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re")
for proj in ["yibi-mvp", "nextrek-cli", "yibi-stack"]:
    items = json.loads((R / f"p02/{proj}.specs.openspec.json").read_text())["items"]
    ph = [i["id"] for i in items if any("placeholder" in x["message"] for x in i["issues"])]
    rs = [i["id"] for i in items if any(x["message"].startswith("Spec must have a Requirements section") for x in i["issues"])]
    pb = [i["id"] for i in items if any("too brief" in x["message"] for x in i["issues"])]
    print(proj, "placeholder:", len(ph), ph[:4], "| requirements-section-missing:", rs, "| brief:", pb)
raw = (R / "p08/changes.openspec.json").read_text()
doc = json.loads(raw)
seen = set()
for it in doc["items"]:
    for x in it["issues"]:
        k = tuple(x.keys())
        if k not in seen:
            seen.add(k)
            print("issue key order:", k, "e.g.", x["message"][:60])
raw = (R / "p08/specs.openspec.json").read_text()
for it in json.loads(raw)["items"]:
    for x in it["issues"]:
        k = tuple(x.keys())
        if k not in seen:
            seen.add(k)
            print("issue key order:", k, "e.g.", x["message"][:60])
