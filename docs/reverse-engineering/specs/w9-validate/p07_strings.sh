#!/bin/bash
# W9 p07：oracle 二進位檔中與 validate 相關的訊息字串
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
strings -n 6 /Applications/Spectra.app/Contents/MacOS/spectra > "$W/strings.txt"
grep -n -i -e 'Invalid format' -e 'Parse error' -e 'No delta specs' -e 'Archive would refuse' \
  -e 'delta-spec syntax' -e 'Validation failed' -e ' valid' -e 'invalid' -e 'warn: ' -e 'error: ' \
  -e 'No requirements found' -e 'Missing ## ' -e 'Invalid requirement header' -e 'at least one operation' \
  -e 'MODIFIED operation' -e 'scenario' -e 'Duplicate' -e 'RENAMED' "$W/strings.txt" | cut -c1-300 | head -150
