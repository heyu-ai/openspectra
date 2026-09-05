#!/usr/bin/env python3
"""Turn fetched upstream release/issue JSON into a triage digest.

Pure transformation: reads JSON files produced by `gh` in the CI step (no
network here, so it is unit-testable against fixtures) and writes a Markdown
digest plus an item count. The digest is what the `upstream-watch` workflow
posts as a GitHub issue for a human (or Claude on demand) to triage.

Two upstreams, two distinct concerns for openspectra:

* ``kaochenlong/spectra-app`` is the **oracle** openspectra reverse-engineers.
  A new release means the CLI surface / drift heuristics may have moved and the
  golden calibration + ``docs/reverse-engineering/*`` may be stale. Low volume,
  so every item is included.
* ``Fission-AI/OpenSpec`` is the **format authority** for the OpenSpec
  conventions that ``validate`` / ``archive`` delta-merge implement. High
  volume and noisy, so closed issues are filtered to format-relevant ones by
  keyword; releases are always included.

Usage:
    upstream-watch.py --since 2026-07-11 \
        --oracle-releases o_rel.json --oracle-issues o_iss.json \
        --openspec-releases s_rel.json --openspec-issues s_iss.json \
        --out digest.md

Prints ``ITEM_COUNT=<n>`` as its last stdout line so the workflow can decide
whether to open an issue.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Substrings (case-insensitive) that make an OpenSpec issue relevant to
# openspectra's validate/archive/spec-format surface. Kept deliberately broad;
# tune here as false positives/negatives show up in real digests.
OPENSPEC_KEYWORDS = (
    "validate",
    "archive",
    "spec",
    "capability",
    "delta",
    "scenario",
    "requirement",
    "convention",
    "nested",
    "namespace",
    "path",
    "task",
    "drift",
    "added",
    "modified",
    "removed",
    "renamed",
    "shall",
    "must",
)


def load_json_array(path: str | None) -> list[dict]:
    """Load a JSON array file, tolerating a missing or empty file as []."""
    if not path:
        return []
    p = Path(path)
    if not p.exists():
        return []
    text = p.read_text(encoding="utf-8").strip()
    if not text:
        return []
    data = json.loads(text)
    if not isinstance(data, list):
        raise ValueError(f"{path}: expected a JSON array, got {type(data).__name__}")
    return data


def relevant_issue(issue: dict) -> bool:
    """True if an OpenSpec issue title hits any format-relevance keyword."""
    title = (issue.get("title") or "").lower()
    return any(kw in title for kw in OPENSPEC_KEYWORDS)


def fmt_releases(releases: list[dict]) -> list[str]:
    lines = []
    for r in releases:
        tag = r.get("tagName") or r.get("tag_name") or "?"
        name = r.get("name") or ""
        published = (r.get("publishedAt") or r.get("published_at") or "")[:10]
        url = r.get("url") or r.get("html_url") or ""
        if not name or name == tag or name.startswith(tag):
            label = name or tag
        else:
            label = f"{tag} — {name}"
        lines.append(f"- [ ] **{label}** ({published}) {url}")
    return lines


def fmt_issues(issues: list[dict]) -> list[str]:
    lines = []
    for i in issues:
        num = i.get("number")
        title = i.get("title") or ""
        url = i.get("url") or i.get("html_url") or ""
        closed = (i.get("closedAt") or i.get("closed_at") or "")[:10]
        lines.append(f"- [ ] #{num} {title} (closed {closed}) {url}")
    return lines


def build_digest(
    since: str,
    oracle_releases: list[dict],
    oracle_issues: list[dict],
    openspec_releases: list[dict],
    openspec_issues: list[dict],
) -> tuple[str, int]:
    """Return (markdown, item_count). item_count drives whether to open an issue."""
    openspec_relevant = [i for i in openspec_issues if relevant_issue(i)]
    openspec_filtered_out = len(openspec_issues) - len(openspec_relevant)

    item_count = (
        len(oracle_releases)
        + len(oracle_issues)
        + len(openspec_releases)
        + len(openspec_relevant)
    )

    out: list[str] = []
    out.append(f"Upstream changes since **{since}**. Triage each item as "
               "**adopt / investigate / ignore**; check the box once triaged.")
    out.append("")
    out.append("> Cross-reference against openspectra's command surface, "
               "`docs/reverse-engineering/*.md`, the open RE questions "
               "(#8 / #9 / #11), and the parity gap issues (#55–#65).")
    out.append("")

    out.append("## 🔭 Oracle — `kaochenlong/spectra-app` (v2.3.1 base)")
    out.append("*A change here means the reverse-engineering target moved: "
               "re-probe the CLI surface, re-run golden calibration, and check "
               "`calibration.rs` + the RE write-ups for drift.*")
    out.append("")
    out.append("### Releases")
    out.extend(fmt_releases(oracle_releases) or ["- _(none)_"])
    out.append("")
    out.append("### Closed issues")
    out.extend(fmt_issues(oracle_issues) or ["- _(none)_"])
    out.append("")

    out.append("## 📐 Format authority — `Fission-AI/OpenSpec` (v1.12.0 base)")
    out.append("*A change here may shift the OpenSpec conventions that "
               "`validate` / `archive` delta-merge implement.*")
    out.append("")
    out.append("### Releases")
    out.extend(fmt_releases(openspec_releases) or ["- _(none)_"])
    out.append("")
    out.append("### Closed issues (format-relevant only)")
    out.extend(fmt_issues(openspec_relevant) or ["- _(none)_"])
    if openspec_filtered_out:
        out.append("")
        out.append(f"_{openspec_filtered_out} other closed issue(s) filtered "
                   "out as off-topic (billing, unrelated CLIs, etc.)._")
    out.append("")

    out.append("---")
    out.append("_Generated by `.github/workflows/upstream-watch.yml`. "
               "Close this issue once every item is triaged; the next run "
               "opens a fresh digest._")

    return "\n".join(out) + "\n", item_count


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--since", required=True, help="Window start date (YYYY-MM-DD), for display.")
    ap.add_argument("--oracle-releases")
    ap.add_argument("--oracle-issues")
    ap.add_argument("--openspec-releases")
    ap.add_argument("--openspec-issues")
    ap.add_argument("--out", required=True, help="Path to write the Markdown digest.")
    args = ap.parse_args(argv)

    digest, count = build_digest(
        since=args.since,
        oracle_releases=load_json_array(args.oracle_releases),
        oracle_issues=load_json_array(args.oracle_issues),
        openspec_releases=load_json_array(args.openspec_releases),
        openspec_issues=load_json_array(args.openspec_issues),
    )
    Path(args.out).write_text(digest, encoding="utf-8")
    print(f"ITEM_COUNT={count}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
