#!/usr/bin/env python3
"""印出某 project/scope/id 在三個工具的完整 finding（除錯用）。用法：show_item.py proj scope id"""
import json
import sys
from pathlib import Path

R = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re/p02")
proj, scope, iid = sys.argv[1:4]
for tool in ["openspec", "openspec-strict", "os"]:
    doc = json.loads((R / f"{proj}.{scope}.{tool}.json").read_text())
    for it in doc["items"]:
        if it["id"] == iid:
            print(f"--- {tool} valid={it['valid']}")
            for x in it["issues"]:
                if x["level"] == "INFO" and "very long" in x["message"]:
                    continue
                print(f"  {x['level']} {x['path']} line={x.get('line')} :: {x['message']}")
doc = json.loads((R / f"{proj}.{scope}.oracle.json").read_text())
for it in doc:
    if (it.get("change") or it.get("spec")) == iid:
        print("--- oracle", json.dumps(it, ensure_ascii=False))
