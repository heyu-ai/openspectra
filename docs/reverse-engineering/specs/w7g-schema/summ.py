#!/usr/bin/env python3
# Compact per-case summary of a matrix.py output file.
import re, sys
text = open(sys.argv[1]).read()
for block in text.split('\n########## CASE ')[1:]:
    head, *rest = block.split('\n')
    print('== ' + head)
    cur = None
    for line in rest:
        m = re.match(r'### (ORACLE|OURS): (.*?)\s+\[rc=(\d+)\](.*)', line)
        if m:
            who, cmd, rc, tail = m.groups()
            short = cmd.replace('schema validate m', 'V').replace('--change c1 ', '')
            cur = f'{who[:3]} {short} rc={rc}{tail}'
            print('  ' + cur)
            continue
        if line.startswith('  err| ') or (line.startswith('  out| ') and ('"error"' in line or 'OURS' in (cur or '') or '✓' in line)):
            if '"error"' in line or line.startswith('  err| ') or cur.startswith('OUR') or '✓' in line:
                if 'Schema validation failed' in line and 'V --json' not in cur:
                    continue
                print('      ' + line.strip()[5:])
