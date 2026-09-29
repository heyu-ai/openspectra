#!/usr/bin/env python3
"""W9 p03：逐 item 比對 oracle / OpenSpec 1.13.2 / OpenSpectra 的 validate --json（p02 的輸出）。"""
import json
import sys
from pathlib import Path

R = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re/p02")
FULL = "--full" in sys.argv


def load(p):
    try:
        return json.loads(p.read_text())
    except Exception as e:  # noqa: BLE001
        return {"__error__": str(e)}


def oracle_items(doc, scope):
    key = "change" if scope == "changes" else "spec"
    out = {}
    order = []
    for it in doc:
        out[it[key]] = it
        order.append(it[key])
    return out, order


def v2_items(doc):
    out = {}
    order = []
    for it in doc.get("items", []):
        out[it["id"]] = it
        order.append(it["id"])
    return out, order


for proj in ["yibi-mvp", "nextrek-cli", "yibi-stack"]:
    for scope in ["changes", "specs"]:
        o, oord = oracle_items(load(R / f"{proj}.{scope}.oracle.json"), scope)
        op_doc = load(R / f"{proj}.{scope}.openspec.json")
        ops_doc = load(R / f"{proj}.{scope}.openspec-strict.json")
        os_doc = load(R / f"{proj}.{scope}.os.json")
        op, opord = v2_items(op_doc)
        ops, _ = v2_items(ops_doc)
        osx, osord = v2_items(os_doc)
        ids = sorted(set(o) | set(op) | set(osx))
        print(f"\n######## {proj} --{scope}: oracle={len(o)} openspec={len(op)} openspectra={len(osx)}")
        print(f"  oracle invalid={sum(1 for i in o.values() if not i['valid'])} "
              f"openspec invalid={sum(1 for i in op.values() if not i['valid'])} "
              f"openspec-strict invalid={sum(1 for i in ops.values() if not i['valid'])} "
              f"openspectra invalid={sum(1 for i in osx.values() if not i['valid'])}")
        for miss_name, s in [("oracle", o), ("openspec", op), ("openspectra", osx)]:
            missing = [i for i in ids if i not in s]
            if missing:
                print(f"  MISSING in {miss_name}: {missing}")
        for i in ids:
            ov = o.get(i, {}).get("valid")
            pv = op.get(i, {}).get("valid")
            psv = ops.get(i, {}).get("valid")
            sv = osx.get(i, {}).get("valid")
            pi = [(x["level"], x["path"], x["message"]) for x in op.get(i, {}).get("issues", [])]
            si = [(x["level"], x["path"], x["message"]) for x in osx.get(i, {}).get("issues", [])]
            differs = pv != sv or sorted(pi) != sorted(si)
            if not differs and not FULL:
                continue
            print(f"\n  == {i}: oracle={ov} openspec={pv} strict={psv} openspectra={sv}")
            for x in pi:
                print(f"    OPENSPEC  {x[0]:7} {x[1]} :: {x[2][:260]}")
            for x in si:
                print(f"    OPENSPECTRA {x[0]:7} {x[1]} :: {x[2][:260]}")
            if i in o and (o[i]["errors"] or o[i]["warnings"]):
                for e in o[i]["errors"]:
                    print(f"    ORACLE    error   {e[:200]}")
                for w in o[i]["warnings"]:
                    print(f"    ORACLE    warn    {w[:200]}")
