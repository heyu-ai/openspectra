#!/usr/bin/env python3
"""W9 p07b：在 oracle 二進位檔裡找 validate 訊息片段，印出前後文（raw bytes）。"""
import re

data = open("/Applications/Spectra.app/Contents/MacOS/spectra", "rb").read()
needles = [
    b"delta-spec syntax", b"Invalid requirement header", b"No requirements found", b"Missing ## ",
    b"operation(s) but", b"Archive would refuse", b"No delta specs found", b"Parse error",
    b"Validation failed", b"at least one operation", b"Delta spec must be inside",
    b"Invalid format", b"Duplicate requirement", b"FROM", b"requirement name",
    b"Scenario", b"Missing ## Purpose", b"empty requirement", b"has no scenarios",
]
for n in needles:
    for m in list(re.finditer(re.escape(n), data))[:4]:
        s = max(0, m.start() - 160)
        e = m.end() + 220
        chunk = data[s:e].decode("utf-8", "replace").replace("\n", "\\n")
        print(f"=== {n.decode()} @ {m.start()}\n{chunk}\n")
