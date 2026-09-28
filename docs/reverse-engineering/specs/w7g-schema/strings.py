#!/usr/bin/env python3
# Extract schema-related message fragments from the oracle binary (for leads only; probes are authoritative)
import re
data = open('/Applications/Spectra.app/Contents/MacOS/spectra', 'rb').read()
pat = re.compile(rb'(is invalid|Schema not found|not found in project|Forked|already exists|uplicate|ycle|ircular|Schema validation|Invalid schema|emplate|must not|must be|artifactCount|schema\.yaml|Unknown artifact|unknown artifact|references|depends on|self)')
seen = set()
for m in pat.finditer(data):
    s = max(0, m.start() - 120); e = m.end() + 120
    frag = data[s:e].decode('utf-8', 'replace')
    frag = re.sub(r'[\x00-\x1f�]', '|', frag)
    key = frag[100:160]
    if key in seen: continue
    seen.add(key)
    print(repr(frag))
