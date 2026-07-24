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
import os
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


def snapshot_tree(root: Path) -> dict[str, bytes]:
    """整棵樹的 relpath → bytes（不排除任何東西）。"""
    return {
        p.relative_to(root).as_posix(): p.read_bytes()
        for p in sorted(root.rglob("*"))
        if p.is_file()
    }


def run_update_sandbox(
    spectra: str, tmp: Path, tool_id: str, detect_dir: str, spec_dir: str | None
) -> tuple[Path, dict[str, bytes], str]:
    """建沙盒 → init → 快照 → mkdir 偵測目錄 → update → 差分。

    寫檔集合是 **量出來的**（update 前後樹狀差分），不是「全部檔案扣掉三個
    寫死的 baseline 路徑」。舊版那種寫法有兩個 fail-open：init 若新增一個
    不在寫死清單裡的檔案，會被誤記成 update 的模板並進 manifest 與 golden；
    而 update 若動到 baseline 三件套，也不會有人發現（PR #86 review 由
    Codex 與 Claude/code-reviewer 各自指出）。
    """
    tag = "default" if spec_dir is None else "token"
    root = tmp / f"{tool_id}-{tag}"
    root.mkdir(parents=True)
    init_argv = [spectra, "init", str(root), "--no-color"]
    if spec_dir is not None:
        init_argv += ["--dir", spec_dir]
    r = run(init_argv)
    if r.returncode != 0:
        fail(f"{tool_id}/{tag}: init failed: {r.stderr.strip()}")

    before = snapshot_tree(root)
    (root / detect_dir).mkdir(parents=True)
    r = run([spectra, "update", str(root), "--no-color"])
    if r.returncode != 0:
        fail(f"{tool_id}/{tag}: update failed: {r.stderr.strip()}")
    expected = f"✓ Updated instruction files for: {tool_id}\n"
    if r.stdout != expected:
        fail(f"{tool_id}/{tag}: stdout {r.stdout!r} != expected {expected!r}")
    after = snapshot_tree(root)

    # update 不得改動 init 已經放好的任何檔案。
    for rel, content in before.items():
        if rel not in after:
            fail(f"{tool_id}/{tag}: update deleted a pre-existing file: {rel}")
        if after[rel] != content:
            fail(f"{tool_id}/{tag}: update modified a pre-existing file: {rel}")

    written = {rel: b for rel, b in after.items() if rel not in before}
    return root, written, r.stdout


def probe_file_kind(spectra: str, tmp: Path, tool_id: str, detect_dir: str, relpath: str) -> str:
    """實測某個檔案是 Managed（保留 marker 區塊外的內容）還是 Plain（整檔覆寫）。

    做法：先讓 oracle 寫一次，在檔尾附加 sentinel，再跑一次 update，看
    sentinel 還在不在。這取代了舊版「模板文字以 START marker 開頭就當
    Managed」的猜測——該猜測從未對 merge 行為驗證過（capture 只跑全新沙盒，
    兩種 kind 的首次寫入位元組相同），實測發現它把 10 個
    `.kilocode/workflows/*.md` 誤判成 Managed，而 oracle 其實整檔覆寫
    （PR #86 review 由 Claude/silent-failure-hunter 指出並經 lead 重現）。
    """
    sentinel = "ZZ_KIND_PROBE_SENTINEL_ZZ"
    # sha256 而非 hash()：CPython 3.3 起字串 hash 每個 interpreter 執行都不同，
    # 兩個 relpath 撞號時沙盒目錄會重疊（mkdir 直接炸，但理由完全看不出來）。
    digest = hashlib.sha256(relpath.encode("utf-8")).hexdigest()[:8]
    root = tmp / f"kindprobe-{tool_id}-{digest}"
    root.mkdir(parents=True)
    r = run([spectra, "init", str(root), "--no-color"])
    if r.returncode != 0:
        fail(f"{tool_id}: kind-probe init failed: {r.stderr.strip()}")
    (root / detect_dir).mkdir(parents=True, exist_ok=True)
    if run([spectra, "update", str(root), "--no-color"]).returncode != 0:
        fail(f"{tool_id}: kind-probe seed update failed")

    target = root / relpath
    if not target.is_file():
        fail(f"{tool_id}: kind-probe target missing after seed update: {relpath}")
    target.write_text(target.read_text() + f"\n{sentinel}\n", encoding="utf-8")
    if run([spectra, "update", str(root), "--no-color"]).returncode != 0:
        fail(f"{tool_id}: kind-probe second update failed")
    return "Managed" if sentinel in target.read_text(encoding="utf-8") else "Plain"


def blob_ext(relpath: str) -> str:
    suffix = Path(relpath).suffix
    return suffix.lstrip(".") if suffix else "txt"


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


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

    # ---- FileKind：對 oracle 實測，不從模板文字猜 ----
    # 只有「模板本身是完整 marker 區塊」的檔案才需要問；其餘必然是整檔覆寫。
    # 但「是 marker 區塊」不蘊含「oracle 會做 merge」——10 個
    # `.kilocode/workflows/*.md` 正是反例，所以逐一 probe。
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
            kinds[(tool_id, rel)] = probe_file_kind(
                spectra, tmp, tool_id, detect_dir, rel
            )
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
        # 不是失敗，但一定要顯示——這正是「用文字前綴猜」會漏掉的那一類。
        for tool_id, rel in plain_marker_files:
            print(f"     [note] {tool_id}:{rel} starts with the START marker but is Plain")
    # 兩個方向都要斷言。只檢查 `not managed` 是**單邊**守衛：一個退化成
    # 「sentinel 永遠存活」的 probe（stale read、目標路徑不再被改寫、未來的
    # 工具組合觸發抑制怪癖）會通過它，然後把 22 個 marker 形狀的檔案全部寫成
    # Managed，靜默還原 6a5bee4 修掉的缺陷。零命中的守衛在證明它會對已知壞
    # 輸入失敗之前沒有資訊量。（PR #86 round-2, Claude/silent-failure-hunter）
    if not managed:
        fail("kind probe found no Managed file -- the probe is not discriminating")
    if not plain_marker_files:
        fail(
            "kind probe found no marker-shaped Plain file -- the probe is not "
            "discriminating (kilocode's workflows must land here)"
        )
    if (len(managed), len(plain_marker_files)) != (12, 10):
        fail(
            f"probed kind split drifted: {len(managed)} Managed / "
            f"{len(plain_marker_files)} marker-shaped Plain, pinned at 12 / 10. "
            "If the oracle really changed, update this pin AND update.md."
        )

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
    # 這個檢查驗的是「已知工具的順序與存在」，**不是集合封閉性**：偵測目錄與
    # expected 字串都由本檔的 TOOLS 產生，所以 oracle 未來「新增」一個工具時，
    # 它不會被建目錄、不會被偵測、也不會出現在 stdout，本檢查照樣 exit 0。
    # 經 probe 確認 CLI 無從列舉 registry（`init --tools bogus-tool-xyz` 會 exit 0
    # 並印 `Generated files for: bogus-tool-xyz`，不拒絕未知 id），所以這裡如實
    # 標示限制，而不是假裝有涵蓋（PR #86 review，Claude/silent-failure-hunter）。
    if len(TOOLS) != 23:
        fail(
            f"TOOLS has {len(TOOLS)} entries, pinned at 23. If the oracle really "
            "gained or lost a tool, update this pin AND update.md's detection matrix."
        )

    # codex×gemini 抑制怪癖：gemini 在場時 codex 只寫 AGENTS.md
    # （.agents/skills/* 整組不寫）。port 依賴這條規則，capture 時一併驗證
    # oracle 仍是這個行為。
    quirk_root = tmp / "codex-gemini"
    quirk_root.mkdir()
    r = run([spectra, "init", str(quirk_root), "--no-color"])
    if r.returncode != 0:
        fail(f"codex-gemini: init failed: {r.stderr.strip()}")
    # 與 run_update_sandbox 同樣用 before/after 差分，而不是扣掉三個寫死的
    # baseline 路徑——後者在 init 新增或改名檔案時會為了無關的理由爆掉。
    quirk_before = set(snapshot_tree(quirk_root))
    (quirk_root / ".agents").mkdir()
    (quirk_root / ".gemini").mkdir()
    r = run([spectra, "update", str(quirk_root), "--no-color"])
    if r.stdout != "✓ Updated instruction files for: gemini, codex\n":
        fail(f"codex-gemini: unexpected stdout {r.stdout!r}")
    quirk_files = set(snapshot_tree(quirk_root)) - quirk_before
    expected_quirk = {rel for rel, _ in per_tool["gemini"]} | {"AGENTS.md"}
    if quirk_files != expected_quirk:
        fail(
            "codex-gemini suppression quirk drifted: "
            f"{sorted(quirk_files ^ expected_quirk)}"
        )
    print("[OK] codex-gemini suppression quirk verified")

    # blob 檔名是 sha256 的前 12 個 hex；兩個不同模板若前綴與副檔名都相同，
    # 第二個 write_text 會靜默蓋掉第一個，於是某個工具的 manifest 會
    # include_str! 到錯的模板。機率極低，但這支腳本的定位是 fail-loud
    # 驗證契約，不能留靜默失敗面。
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


if __name__ == "__main__":
    main()
