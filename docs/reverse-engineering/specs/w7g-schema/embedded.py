#!/usr/bin/env python3
# Locate the embedded built-in schema.yaml texts in the oracle binary; print their artifact id order
# and the apply block, to compare with what `schema fork` writes.
import re
d = open('/Applications/Spectra.app/Contents/MacOS/spectra', 'rb').read()
for name in (b'spec-driven', b'no-spec'):
    for m in re.finditer(b'name: ' + name + b'\n', d):
        i = m.start()
        chunk = d[i:i + 20000]
        end = chunk.find(b'\x00')
        chunk = chunk[:end if end > 0 else 20000]
        ids = re.findall(rb'\n\s*- id: ([a-z-]+)', chunk)
        ap = chunk.find(b'\napply:')
        print('====', name, 'at', i, 'ids', ids)
        print(repr(chunk[:200]))
        print('apply block:', repr(chunk[ap:ap + 120]))
