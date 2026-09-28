"""W12 手動 mutation 檢查：每個 case 只改一處（anchor 必須恰好出現一次），跑指定測試必須失敗，
再以反向替換還原並 touch。任何 anchor 不唯一、突變存活、還原後內容不符都 [FAIL]。"""
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

WT = Path("/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands")
CASES = [
    ("M1 supersedes 以第一個 / 切開", "crates/spectra-core/src/decisions.rs",
     "line[SUPERSEDES_FIELD.len()..].split_once('/')?", "line[SUPERSEDES_FIELD.len()..].rsplit_once('/')?",
     ["-p", "spectra-core", "--lib", "decisions::tests::supersedes_field_takes_the_first_matching_line_only"]),
    ("M2 重複目標只標最後一筆", "crates/spectra-core/src/decisions.rs",
     "        index.insert((d.change.clone(), d.heading.clone()), i);",
     "        index.entry((d.change.clone(), d.heading.clone())).or_insert(i);",
     ["-p", "spectra-core", "--lib", "decisions::tests::only_the_last_duplicate_target_is_marked_superseded"]),
    ("M3 封存名稱長度門檻 > 11", "crates/spectra-core/src/decisions.rs",
     "bytes.len() > 11 && bytes[4] == b'-'", "bytes.len() >= 11 && bytes[4] == b'-'",
     ["-p", "spectra-core", "--lib", "decisions::tests::archived_directory_names_follow_the_oracle_prefix_rules"]),
    ("M4 fence 切換", "crates/spectra-core/src/decisions.rs",
     "        if toggles_fence(line) {\n            in_fence = !in_fence;",
     "        if toggles_fence(line) {\n            in_fence = false;",
     ["-p", "spectra-core", "--lib", "decisions::tests::fences_hide_structure_but_stay_in_the_rationale"]),
    ("M5 區段結束需 `## `", "crates/spectra-core/src/decisions.rs",
     "            if line.starts_with(\"## \") {", "            if line.starts_with(\"##\") {",
     ["-p", "spectra-core", "--lib", "decisions::tests::only_a_level_two_heading_with_a_space_ends_the_section"]),
    ("M6 keyword 也搜 rationale", "crates/spectra-core/src/decisions.rs",
     "                || d.rationale.to_lowercase().contains(&needle)",
     "                || d.change.to_lowercase().contains(&needle)",
     ["-p", "spectra-core", "--lib", "decisions::tests::collect_orders_active_by_mtime_then_archived_and_resolves_supersession"]),
    ("M7 封存依目錄名稱遞增", "crates/spectra-core/src/decisions.rs",
     "    archived.sort();", "    archived.sort_by(|a, b| b.cmp(a));",
     ["-p", "spectra-core", "--lib", "decisions::tests::collect_orders_active_by_mtime_then_archived_and_resolves_supersession"]),
    ("M8 --item-type change 只找 change", "crates/spectra-core/src/show.rs",
     "        Some(\"change\") => {\n            change_view(cfg, item)?",
     "        Some(\"change\") => {\n            spec_view(cfg, item)?",
     ["-p", "spectra-core", "--lib", "show::tests::item_type_restricts_the_lookup_and_rejects_unknown_types_first"]),
    ("M9 supersedes 黃色", "crates/spectra-cli/src/main.rs",
     "colorize(\"supersedes\", \"33\", use_color)", "colorize(\"supersedes\", \"31\", use_color)",
     ["-p", "spectra-cli", "--bin", "spectra", "decisions_human_matches_the_oracle_tty_bytes"]),
    ("M10 demo schema 固定 spec-driven", "crates/spectra-core/src/demo.rs",
     "        schema: Some(crate::schema::SCHEMA_NAME.to_string()),",
     "        schema: crate::schema::configured_schema_name(cfg),",
     ["-p", "spectra-core", "--lib", "demo::tests::schema_is_always_spec_driven_even_with_a_custom_project_schema"]),
    ("M11 缺名稱錯誤訊息", "crates/spectra-cli/src/main.rs",
     "anyhow::bail!(\"Please specify an item name.\");", "anyhow::bail!(\"Please specify an item.\");",
     ["-p", "spectra-cli", "--test", "show_demo_feedback_integration", "show_without_item_is_a_runtime_error"]),
]


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main() -> int:
    only = set(sys.argv[1:])
    ok = True
    for label, rel, anchor, mutant, test in CASES:
        if only and label.split()[0] not in only:
            continue
        path = WT / rel
        original = path.read_text()
        before = sha(path)
        if original.count(anchor) != 1:
            print(f"[FAIL] {label}: anchor occurs {original.count(anchor)} times")
            return 1
        path.write_text(original.replace(anchor, mutant))
        try:
            p = subprocess.run(["cargo", "test", "--manifest-path", str(WT / "Cargo.toml"), *test],
                               capture_output=True, text=True)
            ran = "running 0 tests" not in p.stdout and " 0 passed; 0 failed" not in p.stdout
            killed = p.returncode != 0 and ("FAILED" in p.stdout or "panicked" in p.stdout)
            unviable = "error[" in p.stderr
        finally:
            restored = path.read_text().replace(mutant, anchor)
            path.write_text(restored)
            os.utime(path, None)
        if sha(path) != before:
            print(f"[FAIL] {label}: restore mismatch")
            return 1
        if unviable:
            print(f"[FAIL] {label}: mutant does not compile\n{p.stderr[-1500:]}")
            ok = False
        elif not ran:
            print(f"[FAIL] {label}: test filter matched nothing")
            ok = False
        elif killed:
            print(f"[KILLED] {label}")
        else:
            print(f"[SURVIVED] {label}\n{p.stdout[-800:]}")
            ok = False
        time.sleep(1)
    return 0 if ok else 1


sys.exit(main())
