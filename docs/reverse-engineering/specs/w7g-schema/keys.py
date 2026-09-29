#!/usr/bin/env python3
# Print the key skeleton (top-level keys, artifact ids, per-artifact key order) of schema.yaml files
import sys, re
for p in sys.argv[1:]:
    t = open(p, 'rb').read()
    print('=====', p, len(t), 'bytes, ends with', repr(t[-20:]), 'CRLF' if b'\r\n' in t else 'LF')
    for i, line in enumerate(t.decode().split('\n'), 1):
        if re.match(r'^[A-Za-z_]+:', line) or re.match(r'^\s*- id:', line) or re.match(r'^  ?  ?[a-z_]+:', line):
            print(f'{i:4}: {line[:100]}')
