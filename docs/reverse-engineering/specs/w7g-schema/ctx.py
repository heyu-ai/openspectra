#!/usr/bin/env python3
# Print raw context around given byte strings in the oracle binary
import sys
d = open('/Applications/Spectra.app/Contents/MacOS/spectra', 'rb').read()
for k in sys.argv[1:]:
    kb = k.encode()
    i = d.find(kb)
    print('=====', k, i)
    if i >= 0:
        print(repr(d[i - 300:i + 600]))
