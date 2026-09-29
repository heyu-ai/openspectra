"""把 `one()` 形式的探針輸出摘要成 (heading, rationale[, extra]) 列表。"""
import json
import sys

t = open(sys.argv[1]).read()
keys = sys.argv[2:] or ["heading", "rationale"]
for block in t.split("======== case ")[1:]:
    name, rest = block.split("\n", 1)
    parts = rest.split("### ORACLE: decisions --json\n", 1)
    if len(parts) < 2:
        print(name, "RAW", repr(rest[:400]))
        continue
    js = parts[1].rsplit("[rc=", 1)[0]
    try:
        d = json.loads(js)
        print(name, [tuple(x[k] for k in keys) for x in d])
    except Exception:
        print(name, "RAW", repr(parts[1][:400]))
