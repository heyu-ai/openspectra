#!/usr/bin/env python3
"""Capture `spectra update` instruction templates from the reference binary.

macOS only（oracle 是 arm64 app）。對 23 個 AI 工具各跑兩個沙盒：

- default 沙盒：`spectra init`（spec_dir=openspec）→ 建偵測目錄 → `spectra update`
- token 沙盒：`spectra init --dir <token>` → 同上

兩份輸出差分還原 `{{SPEC_DIR}}` placeholder（直接搜 "openspec" 會誤傷
"OpenSpec" 品牌字串；token 是唯一字串所以替換位置無歧義）。

這是 verification contract 不是印表機：
- template 以 openspec 代回後必須與 default 沙盒逐位元一致，否則 [FAIL]
  exit 2 並保留兩個沙盒供檢查。
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
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# Registry order 與偵測目錄逐一 probe 自 oracle 2.3.1（見
# docs/reverse-engineering/update.md 的偵測矩陣）。順序即 stdout 訊息順序。
TOOLS = [
    ("claude", ".claude"),
    ("cursor", ".cursor"),
    ("windsurf", ".windsurf"),
    ("cline", ".clinerules"),
    ("gemini", ".gemini"),
    ("github-copilot", ".github/prompts"),
    ("kiro", ".kiro"),
    ("roocode", ".roo"),
    ("continue", ".continue"),
    ("opencode", ".opencode"),
    ("codebuddy", ".codebuddy"),
    ("costrict", ".cospec"),
    ("antigravity", ".agent"),
    ("auggie", ".augment"),
    ("amazon-q", ".amazonq"),
    ("kilocode", ".kilocode"),
    ("factory", ".factory"),
    ("iflow", ".iflow"),
    ("qoder", ".qoder"),
    ("qwen", ".qwen"),
    ("codex", ".agents"),
    ("crush", ".crush"),
    ("trae", ".trae"),
]

TOKEN = "zzspecdirtokenzz"
PLACEHOLDER = "{{SPEC_DIR}}"
# oracle 2.3.1 的輸出本身含有「未展開的字面 {{SPEC_DIR}}」（cursor
# spectra-ask.md 的 frontmatter description，oracle 漏代換的 bug）。template
# 先把這種字面值跳脫成 RAW_PLACEHOLDER，render 時再還原，逐位元保留 oracle
# 的 bug。
RAW_PLACEHOLDER = "{{RAW_SPEC_DIR}}"
DEFAULT_SPEC_DIR = "openspec"
MARKER_START = "<!-- SPECTRA:START"
SETTINGS_RELPATH = ".claude/settings.json"


def fail(msg: str) -> None:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(2)


def run(argv: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, capture_output=True, text=True)


def oracle_version(spectra: str) -> str:
    out = run([spectra, "--version"]).stdout.split()
    # "spectra 2.3.1 (Apple Silicon)" → "2.3.1"
    if len(out) < 2:
        fail(f"cannot parse --version output: {out}")
    return out[1]


def capture_tree(root: Path, spec_dir: str) -> dict[str, bytes]:
    """所有 update 產出的檔案（排除 init 的 baseline 三件套）。"""
    baseline = {
        ".spectra.yaml",
        ".gitignore",
        f"{spec_dir}/config.yaml",
    }
    files: dict[str, bytes] = {}
    for p in sorted(root.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(root).as_posix()
        if rel in baseline:
            continue
        files[rel] = p.read_bytes()
    return files


def run_update_sandbox(
    spectra: str, tmp: Path, tool_id: str, detect_dir: str, spec_dir: str | None
) -> tuple[Path, dict[str, bytes], str]:
    """建沙盒 → init → mkdir 偵測目錄 → update。回傳 (root, tree, stdout)。"""
    tag = "default" if spec_dir is None else "token"
    root = tmp / f"{tool_id}-{tag}"
    root.mkdir(parents=True)
    init_argv = [spectra, "init", str(root), "--no-color"]
    if spec_dir is not None:
        init_argv += ["--dir", spec_dir]
    r = run(init_argv)
    if r.returncode != 0:
        fail(f"{tool_id}/{tag}: init failed: {r.stderr.strip()}")
    (root / detect_dir).mkdir(parents=True)
    r = run([spectra, "update", str(root), "--no-color"])
    if r.returncode != 0:
        fail(f"{tool_id}/{tag}: update failed: {r.stderr.strip()}")
    expected = f"✓ Updated instruction files for: {tool_id}\n"
    if r.stdout != expected:
        fail(f"{tool_id}/{tag}: stdout {r.stdout!r} != expected {expected!r}")
    return root, capture_tree(root, spec_dir or DEFAULT_SPEC_DIR), r.stdout


def blob_ext(relpath: str) -> str:
    suffix = Path(relpath).suffix
    return suffix.lstrip(".") if suffix else "txt"


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--spectra-bin",
        default="/Applications/Spectra.app/Contents/MacOS/spectra",
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

    # tool_id → [(relpath, template_text)]；blob 去重表 sha → (filename, text)
    per_tool: dict[str, list[tuple[str, str]]] = {}
    blobs: dict[str, tuple[str, str]] = {}
    golden_rows: list[tuple[str, str, str]] = []

    for tool_id, detect_dir in TOOLS:
        _, default_tree, _ = run_update_sandbox(
            spectra, tmp, tool_id, detect_dir, None
        )
        _, token_tree, _ = run_update_sandbox(
            spectra, tmp, tool_id, detect_dir, TOKEN
        )
        if set(default_tree) != set(token_tree):
            fail(
                f"{tool_id}: file sets differ between spec_dirs: "
                f"{sorted(set(default_tree) ^ set(token_tree))}"
            )
        if not default_tree:
            fail(f"{tool_id}: update wrote no files")
        entries: list[tuple[str, str]] = []
        for rel in sorted(default_tree):
            token_text = token_tree[rel].decode("utf-8")
            if RAW_PLACEHOLDER in token_text:
                fail(f"{tool_id}:{rel}: escape token collides with content")
            template = token_text.replace(PLACEHOLDER, RAW_PLACEHOLDER).replace(
                TOKEN, PLACEHOLDER
            )
            # round-trip 驗證：template 展開回 openspec 必須逐位元等於
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
                (tool_id, rel, hashlib.sha256(default_tree[rel]).hexdigest())
            )
        per_tool[tool_id] = entries
        print(f"[OK] {tool_id}: {len(entries)} files")

    # registry 順序驗證：全工具沙盒的訊息必須照 TOOLS 順序列出全部 id。
    all_root = tmp / "all-tools"
    all_root.mkdir()
    r = run([spectra, "init", str(all_root), "--no-color"])
    if r.returncode != 0:
        fail(f"all-tools: init failed: {r.stderr.strip()}")
    for _, detect_dir in TOOLS:
        (all_root / detect_dir).mkdir(parents=True, exist_ok=True)
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

    # codex×gemini 抑制怪癖：gemini 在場時 codex 只寫 AGENTS.md
    # （.agents/skills/* 整組不寫）。port 依賴這條規則，capture 時一併驗證
    # oracle 仍是這個行為。
    quirk_root = tmp / "codex-gemini"
    quirk_root.mkdir()
    r = run([spectra, "init", str(quirk_root), "--no-color"])
    if r.returncode != 0:
        fail(f"codex-gemini: init failed: {r.stderr.strip()}")
    (quirk_root / ".agents").mkdir()
    (quirk_root / ".gemini").mkdir()
    r = run([spectra, "update", str(quirk_root), "--no-color"])
    if r.stdout != "✓ Updated instruction files for: gemini, codex\n":
        fail(f"codex-gemini: unexpected stdout {r.stdout!r}")
    quirk_files = set(capture_tree(quirk_root, DEFAULT_SPEC_DIR))
    expected_quirk = {rel for rel, _ in per_tool["gemini"]} | {"AGENTS.md"}
    if quirk_files != expected_quirk:
        fail(
            "codex-gemini suppression quirk drifted: "
            f"{sorted(quirk_files ^ expected_quirk)}"
        )
    print("[OK] codex-gemini suppression quirk verified")

    # ---- 寫出產物（全部驗證通過之後才動 repo）----
    if assets_dir.exists():
        shutil.rmtree(assets_dir)
    assets_dir.mkdir(parents=True)
    for _, (name, text) in sorted(blobs.items()):
        (assets_dir / name).write_text(text, encoding="utf-8")

    lines = [
        "//! @generated by scripts/capture-update-templates.py against",
        f"//! the reference binary v{version} -- do not edit by hand.",
        "//!",
        "//! Registry order and detection directories are oracle behavior;",
        "//! see docs/reverse-engineering/update.md.",
        "",
        "use crate::update::{FileKind, FileSpec, ToolDef};",
        "",
        "pub static TOOLS: &[ToolDef] = &[",
    ]
    for tool_id, detect_dir in TOOLS:
        lines.append("    ToolDef {")
        lines.append(f"        id: {rust_str(tool_id)},")
        lines.append(f"        detect_dir: {rust_str(detect_dir)},")
        lines.append("        files: &[")
        for rel, blob_name in per_tool[tool_id]:
            if rel == SETTINGS_RELPATH:
                kind = "FileKind::ClaudeSettings"
            elif blobs_text_starts_with_marker(blobs, blob_name):
                kind = "FileKind::Managed"
            else:
                kind = "FileKind::Plain"
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
        f.write("# tool\trelpath\tsha256(bytes with spec_dir=openspec)\n")
        for tool_id, rel, sha in golden_rows:
            f.write(f"{tool_id}\t{rel}\t{sha}\n")

    print(
        f"[OK] {len(blobs)} unique blobs -> {assets_dir}\n"
        f"[OK] manifest -> {manifest_path}\n"
        f"[OK] golden ({len(golden_rows)} rows) -> {golden_path}"
    )
    if not args.keep_tmp:
        shutil.rmtree(tmp)


def blobs_text_starts_with_marker(
    blobs: dict[str, tuple[str, str]], blob_name: str
) -> bool:
    for name, text in blobs.values():
        if name == blob_name:
            return text.startswith(MARKER_START)
    fail(f"internal: blob {blob_name} not found")
    return False  # unreachable


if __name__ == "__main__":
    main()
