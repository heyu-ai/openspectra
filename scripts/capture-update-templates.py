#!/usr/bin/env python3
"""Capture `spectra update` instruction templates from the reference binary.

macOS only（oracle 是 arm64 app）。對 6 個 AI 工具各跑兩個沙盒：

- default/token spec_dir 的 `init --tools` 輸出差分還原 `{{SPEC_DIR}}`
  placeholder（token 是唯一字串所以替換位置無歧義）。

v3.0.0 行為變更：tool 偵測從 detect_dir 改為 skills 目錄結構。`init --tools`
直接生成 tool 檔案，`update` 用 skills 目錄觸發重新生成。此版本 capture 以
`init --tools <tool>` 減去 bare `init` 的差分取得 tool-specific 模板，再以
`update` 驗證冪等性。

這是 verification contract 不是印表機：
- template 以 docs/spectra 代回後必須與 default 沙盒逐位元一致，否則 [FAIL]
  exit 2 並保留沙盒供檢查。
- 每個工具的 update stdout 必須逐字等於預期訊息，否則 [FAIL]。
- 最後用全工具沙盒驗證 registry 順序訊息，不符 [FAIL]。

產出（寫進 repo，之後由 CI 驗證、不需要 oracle）：
- crates/spectra-core/assets/update/<sha12>.<ext>  去重後的 template blobs
- crates/spectra-core/src/update_manifest.rs       @generated registry
- docs/reverse-engineering/golden/update-trees-<ver>.tsv
  （tool, relpath, sha256(default 展開後 bytes)——整合測試對照用）

Usage:
  scripts/capture-update-templates.py [--spectra-bin PATH] [--keep-tmp]
"""

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# Registry order 與偵測目錄逐一 probe 自 oracle 3.0.0（見
# docs/reverse-engineering/update.md 的偵測矩陣）。順序即 stdout 訊息順序。
# v3.0.0 起 detect_dir 僅供 openspectra port 回退相容——oracle 自身已改用
# skills 目錄結構偵測。
TOOLS = [
    ("antigravity", ".agent"),
    ("claude", ".claude"),
    ("codex", ".agents"),
    ("cursor", ".cursor"),
    ("github-copilot", ".github"),
    ("junie", ".junie"),
]

TOKEN = "zzspecdirtokenzz"
PLACEHOLDER = "{{SPEC_DIR}}"
# oracle 輸出本身可能含有未展開的字面 {{SPEC_DIR}}（oracle 漏代換的 bug）。
# template 先把這種字面值跳脫成 RAW_PLACEHOLDER，render 時再還原，逐位元保留
# oracle 的 bug。
RAW_PLACEHOLDER = "{{RAW_SPEC_DIR}}"
# v3.0.0 的預設 spec_dir 是 docs/spectra。golden TSV 的 SHA 對照值以此為準。
DEFAULT_SPEC_DIR = "docs/spectra"
MARKER_START = "<!-- SPECTRA:START"
SETTINGS_RELPATH = ".claude/settings.json"


def fail(msg: str) -> None:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(2)


def run(argv: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, capture_output=True, text=True)


def oracle_version(spectra: str) -> str:
    out = run([spectra, "--version"]).stdout.split()
    if len(out) < 2:
        fail(f"cannot parse --version output: {out}")
    return out[1]


def snapshot_tree(root: Path) -> dict[str, bytes]:
    """整棵樹的 relpath -> bytes（不排除任何東西）。"""
    return {
        p.relative_to(root).as_posix(): p.read_bytes()
        for p in sorted(root.rglob("*"))
        if p.is_file()
    }


def init_sandbox(
    spectra: str,
    tmp: Path,
    tag: str,
    spec_dir: str | None,
    tool_id: str | None = None,
) -> tuple[Path, dict[str, bytes]]:
    """建沙盒 -> init（可含 --tools）-> 快照。"""
    root = tmp / tag
    root.mkdir(parents=True)
    init_argv = [spectra, "init"]
    if tool_id is not None:
        init_argv += ["--tools", tool_id]
    init_argv += [str(root), "--no-color"]
    if spec_dir is not None:
        init_argv += ["--dir", spec_dir]
    r = run(init_argv)
    if r.returncode != 0:
        fail(f"{tag}: init failed: {r.stderr.strip()}")
    return root, snapshot_tree(root)


def tool_specific_files(
    full_tree: dict[str, bytes],
    baseline: dict[str, bytes],
) -> dict[str, bytes]:
    """full_tree 減去 baseline = tool-specific 檔案。"""
    return {rel: b for rel, b in full_tree.items() if rel not in baseline}


def verify_update_idempotent(
    spectra: str,
    root: Path,
    tool_id: str,
    tree: dict[str, bytes],
) -> str:
    """跑 update 驗證冪等性，回傳 stdout。"""
    r = run([spectra, "update", str(root), "--no-color"])
    if r.returncode != 0:
        fail(f"{tool_id}: update failed: {r.stderr.strip()}")
    expected = f"✓ Updated instruction files for: {tool_id}\n"
    if r.stdout != expected:
        fail(f"{tool_id}: stdout {r.stdout!r} != expected {expected!r}")
    after = snapshot_tree(root)
    for rel, content in tree.items():
        if rel not in after:
            fail(f"{tool_id}: update deleted file: {rel}")
        if after[rel] != content:
            fail(f"{tool_id}: update modified file: {rel}")
    return r.stdout


def probe_file_kind(
    spectra: str,
    tmp: Path,
    tool_id: str,
    relpath: str,
) -> str:
    """實測某個檔案是 Managed（保留 marker 區塊外的內容）還是 Plain（整檔覆寫）。

    做法：先讓 oracle 以 init --tools 寫一次，在檔尾附加 sentinel，再跑一次
    update，看 sentinel 還在不在。
    """
    sentinel = "ZZ_KIND_PROBE_SENTINEL_ZZ"
    digest = hashlib.sha256(relpath.encode("utf-8")).hexdigest()[:8]
    root = tmp / f"kindprobe-{tool_id}-{digest}"
    root.mkdir(parents=True)
    r = run([spectra, "init", "--tools", tool_id, str(root), "--no-color"])
    if r.returncode != 0:
        fail(f"{tool_id}: kind-probe init failed: {r.stderr.strip()}")

    target = root / relpath
    if not target.is_file():
        fail(f"{tool_id}: kind-probe target missing after init: {relpath}")
    target.write_text(target.read_text() + f"\n{sentinel}\n", encoding="utf-8")
    if run([spectra, "update", str(root), "--no-color"]).returncode != 0:
        fail(f"{tool_id}: kind-probe update failed")
    return "Managed" if sentinel in target.read_text(encoding="utf-8") else "Plain"


def blob_ext(relpath: str) -> str:
    suffix = Path(relpath).suffix
    return suffix.lstrip(".") if suffix else "txt"


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def blob_text(blobs: dict[str, tuple[str, str]], blob_name: str) -> str:
    for name, text in blobs.values():
        if name == blob_name:
            return text
    fail(f"internal: blob {blob_name} not found")
    return ""  # unreachable


def blob_text_for(
    blobs: dict[str, tuple[str, str]],
    per_tool: dict[str, list[tuple[str, str]]],
    key: tuple[str, str],
) -> str:
    tool_id, rel = key
    for entry_rel, blob_name in per_tool[tool_id]:
        if entry_rel == rel:
            return blob_text(blobs, blob_name)
    fail(f"internal: no blob for {tool_id}:{rel}")
    return ""  # unreachable


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--spectra-bin",
        default=os.environ.get(
            "SPECTRA_BIN", "/Applications/Spectra.app/Contents/MacOS/spectra"
        ),
        help="reference binary path (or set SPECTRA_BIN)",
    )
    ap.add_argument(
        "--keep-tmp", action="store_true", help="keep sandboxes even on success"
    )
    args = ap.parse_args()

    spectra = args.spectra_bin
    if not Path(spectra).is_file():
        fail(f"reference binary not found: {spectra}")

    repo = Path(__file__).resolve().parent.parent
    assets_dir = repo / "crates/spectra-core/assets/update"
    manifest_path = repo / "crates/spectra-core/src/update_manifest.rs"
    golden_dir = repo / "docs/reverse-engineering/golden"

    version = oracle_version(spectra)
    tmp = Path(tempfile.mkdtemp(prefix="spectra-update-capture-"))
    print(f"[OK] oracle {version}; sandboxes under {tmp}")

    # ---- bare init baselines（每個 spec_dir 一個）----
    _, bare_default = init_sandbox(spectra, tmp, "bare-default", None)
    _, bare_token = init_sandbox(spectra, tmp, "bare-token", TOKEN)
    print(f"[OK] bare baselines: {len(bare_default)} / {len(bare_token)} files")

    # ---- 逐工具 capture ----
    per_tool: dict[str, list[tuple[str, str]]] = {}
    blobs: dict[str, tuple[str, str]] = {}
    golden_rows: list[tuple[str, str, str]] = []

    for tool_id, detect_dir in TOOLS:
        default_root, default_full = init_sandbox(
            spectra, tmp, f"{tool_id}-default", None, tool_id
        )
        _, token_full = init_sandbox(
            spectra, tmp, f"{tool_id}-token", TOKEN, tool_id
        )

        default_tree = tool_specific_files(default_full, bare_default)
        token_tree = tool_specific_files(token_full, bare_token)

        if set(default_tree) != set(token_tree):
            fail(
                f"{tool_id}: file sets differ between spec_dirs: "
                f"{sorted(set(default_tree) ^ set(token_tree))}"
            )
        if not default_tree:
            fail(f"{tool_id}: init --tools wrote no tool-specific files")

        # update 冪等性驗證
        verify_update_idempotent(spectra, default_root, tool_id, default_full)

        entries: list[tuple[str, str]] = []
        for rel in sorted(default_tree):
            token_text = token_tree[rel].decode("utf-8")
            if RAW_PLACEHOLDER in token_text:
                fail(f"{tool_id}:{rel}: escape token collides with content")
            template = token_text.replace(PLACEHOLDER, RAW_PLACEHOLDER).replace(
                TOKEN, PLACEHOLDER
            )
            # round-trip 驗證：template 展開回 docs/spectra 必須逐位元等於
            # default 沙盒的實際輸出（capture 的正確性契約）。
            resolved = template.replace(PLACEHOLDER, DEFAULT_SPEC_DIR).replace(
                RAW_PLACEHOLDER, PLACEHOLDER
            )
            if resolved.encode("utf-8") != default_tree[rel]:
                fail(
                    f"{tool_id}:{rel}: round-trip mismatch "
                    f"(sandboxes kept under {tmp})"
                )
            sha = hashlib.sha256(template.encode("utf-8")).hexdigest()
            if sha not in blobs:
                blobs[sha] = (f"{sha[:12]}.{blob_ext(rel)}", template)
            entries.append((rel, blobs[sha][0]))
            golden_rows.append(
                (
                    tool_id,
                    rel,
                    hashlib.sha256(default_tree[rel]).hexdigest(),
                )
            )
        per_tool[tool_id] = entries
        print(f"[OK] {tool_id}: {len(entries)} files")

    # ---- FileKind：對 oracle 實測，不從模板文字猜 ----
    kinds: dict[tuple[str, str], str] = {}
    marker_candidates = 0
    for tool_id, detect_dir in TOOLS:
        for rel, blob_name in per_tool[tool_id]:
            if rel == SETTINGS_RELPATH:
                kinds[(tool_id, rel)] = "ClaudeSettings"
                continue
            if not blob_text(blobs, blob_name).startswith(MARKER_START):
                kinds[(tool_id, rel)] = "Plain"
                continue
            marker_candidates += 1
            kinds[(tool_id, rel)] = probe_file_kind(spectra, tmp, tool_id, rel)
    managed = sorted(k for k, v in kinds.items() if v == "Managed")
    plain_marker_files = sorted(
        k
        for k, v in kinds.items()
        if v == "Plain" and blob_text_for(blobs, per_tool, k).startswith(MARKER_START)
    )
    print(
        f"[OK] file kinds probed: {marker_candidates} marker-shaped templates -> "
        f"{len(managed)} Managed, {len(plain_marker_files)} full-overwrite despite "
        f"looking managed"
    )
    if plain_marker_files:
        for tool_id, rel in plain_marker_files:
            print(f"     [note] {tool_id}:{rel} starts with the START marker but is Plain")
    if not managed:
        fail("kind probe found no Managed file -- the probe is not discriminating")

    # registry 順序驗證：全工具沙盒 update 的訊息必須照 TOOLS 順序列出全部 id。
    all_tools = ",".join(tool_id for tool_id, _ in TOOLS)
    all_root = tmp / "all-tools"
    all_root.mkdir()
    r = run([spectra, "init", "--tools", all_tools, str(all_root), "--no-color"])
    if r.returncode != 0:
        fail(f"all-tools: init failed: {r.stderr.strip()}")
    r = run([spectra, "update", str(all_root), "--no-color"])
    expected = (
        "✓ Updated instruction files for: "
        + ", ".join(tool_id for tool_id, _ in TOOLS)
        + "\n"
    )
    if r.stdout != expected:
        fail(
            f"registry order drifted:\n  oracle: {r.stdout!r}\n"
            f"  pinned: {expected!r}"
        )
    print(f"[OK] registry order verified ({len(TOOLS)} tools)")

    if len(TOOLS) != 6:
        fail(
            f"TOOLS has {len(TOOLS)} entries, pinned at 6. If the oracle really "
            "gained or lost a tool, update this pin AND update.md's detection matrix."
        )

    # blob 檔名碰撞檢查
    blob_names = [name for name, _ in blobs.values()]
    if len(set(blob_names)) != len(blob_names):
        dupes = sorted({n for n in blob_names if blob_names.count(n) > 1})
        fail(f"blob filename collision at 12-hex prefix: {dupes}")

    # ---- 寫出產物（全部驗證通過之後才動 repo）----
    if assets_dir.exists():
        shutil.rmtree(assets_dir)
    assets_dir.mkdir(parents=True)
    for _, (name, text) in sorted(blobs.items()):
        (assets_dir / name).write_text(text, encoding="utf-8")

    # v3.0.0 has no gated files; Gate import is omitted to avoid clippy warning.
    imports = "use crate::update::{FileKind, FileSpec, ToolDef};"
    lines = [
        "//! @generated by scripts/capture-update-templates.py against",
        f"//! the reference binary v{version} -- do not edit by hand.",
        "//!",
        "//! Registry order and detection directories are oracle behavior;",
        "//! see docs/reverse-engineering/update.md.",
        "",
        imports,
        "",
        "pub static TOOLS: &[ToolDef] = &[",
    ]
    for tool_id, detect_dir in TOOLS:
        lines.append("    ToolDef {")
        lines.append(f"        id: {rust_str(tool_id)},")
        lines.append(f"        detect_dir: {rust_str(detect_dir)},")
        lines.append("        files: &[")
        for rel, blob_name in per_tool[tool_id]:
            kind = f"FileKind::{kinds[(tool_id, rel)]}"
            lines.append("            FileSpec {")
            lines.append(f"                relpath: {rust_str(rel)},")
            lines.append(f"                kind: {kind},")
            lines.append(
                "                template: include_str!("
                f'"../assets/update/{blob_name}"),'
            )
            lines.append("            },")
        lines.append("        ],")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    manifest_path.write_text("\n".join(lines), encoding="utf-8")

    golden_dir.mkdir(parents=True, exist_ok=True)
    golden_path = golden_dir / f"update-trees-{version}.tsv"
    with golden_path.open("w", encoding="utf-8") as f:
        f.write(
            f"# tool\trelpath\tsha256(bytes with spec_dir={DEFAULT_SPEC_DIR})\n"
        )
        for tool_id, rel, sha in golden_rows:
            f.write(f"{tool_id}\t{rel}\t{sha}\n")

    print(
        f"[OK] {len(blobs)} unique blobs -> {assets_dir}\n"
        f"[OK] manifest -> {manifest_path}\n"
        f"[OK] golden ({len(golden_rows)} rows) -> {golden_path}"
    )
    if not args.keep_tmp:
        shutil.rmtree(tmp)


if __name__ == "__main__":
    main()
