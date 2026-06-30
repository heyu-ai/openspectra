#!/usr/bin/env bash
# Capture golden `spectra drift --json` output from the closed-source reference
# binary, for calibrating OpenSpectra's constants. macOS only (the reference is
# an arm64 app); the resulting JSON is compared against our Linux build.
#
# Usage:
#   scripts/capture-golden.sh <project-dir> [out-dir]
#
# Writes drift-<change>.json for every active change in <project-dir>.
#
# To reveal the FULL anchor set (resolved anchors are hidden in normal output),
# copy a change's design.md into a throwaway git repo and leave design.md
# UNTRACKED, so `git grep` resolves nothing and every anchor is reported broken.
# See docs/reverse-engineering/drift.md ("Reproducing the oracle").
set -euo pipefail

SPECTRA="${SPECTRA_BIN:-/Applications/Spectra.app/Contents/MacOS/spectra}"
PROJECT="${1:?usage: capture-golden.sh <project-dir> [out-dir]}"
OUT="${2:-golden}"

if [ ! -x "$SPECTRA" ]; then
  echo "[FAIL] reference binary not found/executable: $SPECTRA" >&2
  echo "       set SPECTRA_BIN to its path" >&2
  exit 1
fi
if [ ! -d "$PROJECT/.git" ]; then
  echo "[FAIL] not a git repo: $PROJECT" >&2
  exit 1
fi

mkdir -p "$OUT"
NAMES="$(cd "$PROJECT" && "$SPECTRA" list --changes --json \
  | python3 -c 'import sys,json; [print(c["name"]) for c in json.load(sys.stdin)["changes"]]')"

count=0
while IFS= read -r name; do
  [ -z "$name" ] && continue
  (cd "$PROJECT" && "$SPECTRA" drift "$name" --json) > "$OUT/drift-$name.json"
  count=$((count + 1))
  echo "[OK] $OUT/drift-$name.json"
done <<< "$NAMES"

echo "[OK] captured $count golden file(s) into $OUT/"
