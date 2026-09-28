//! Skill bodies captured byte-exact from the Spectra 3.0.0 oracle.
//! The registry's entries and their order are cross-checked by
//! `scripts/capture-skills.py`. Both the static assets and
//! `docs/reverse-engineering/golden/skills-3.0.0.tsv` are generated
//! artifacts — never hand-edit them.

const SKILLS: &[(&str, &str)] = &[
    ("tdd", include_str!("../assets/skills/tdd.md")),
    ("audit", include_str!("../assets/skills/audit.md")),
    ("apply", include_str!("../assets/skills/apply.md")),
    ("archive", include_str!("../assets/skills/archive.md")),
    ("commit", include_str!("../assets/skills/commit.md")),
    ("debug", include_str!("../assets/skills/debug.md")),
    ("discuss", include_str!("../assets/skills/discuss.md")),
    ("drift", include_str!("../assets/skills/drift.md")),
    ("ingest", include_str!("../assets/skills/ingest.md")),
    ("propose", include_str!("../assets/skills/propose.md")),
    ("analyze", include_str!("../assets/skills/analyze.md")),
    ("verify", include_str!("../assets/skills/verify.md")),
    ("review", include_str!("../assets/skills/review.md")),
    ("sync", include_str!("../assets/skills/sync.md")),
    ("clarify", include_str!("../assets/skills/clarify.md")),
    ("test-scope", include_str!("../assets/skills/test-scope.md")),
    (
        "commit-archive",
        include_str!("../assets/skills/commit-archive.md"),
    ),
    (
        "ingest-plan-mapping",
        include_str!("../assets/skills/ingest-plan-mapping.md"),
    ),
    (
        "ingest-context-mapping",
        include_str!("../assets/skills/ingest-context-mapping.md"),
    ),
    (
        "verify-spec-coverage",
        include_str!("../assets/skills/verify-spec-coverage.md"),
    ),
];

/// `--agent` 可接受的值：與 `update` 的工具 registry 相同，順序即錯誤訊息的列舉順序
/// （oracle 3.0.0：`Supported agents: antigravity, claude, codex, cursor, github-copilot, junie`）。
pub fn supported_agents() -> Vec<&'static str> {
    crate::update::registry().iter().map(|t| t.id).collect()
}

/// 依 agent 渲染 skill body（oracle 3.0.0 probe，見 `artifact-workflow.md` 的 "Agent rendering"）：
///
/// - `{{TOOL}}` → agent id。
/// - `{{SPEC_DIR}}` → spec_dir 去掉結尾 `/` 後補一個 `/`（body 寫的是 `{{SPEC_DIR}}changes/`）。
/// - skill 呼叫語法 `/spectra:<name>`：claude、junie 改成 `/spectra-<name>`，codex 改成
///   `$spectra-<name>`，其餘 agent 維持原樣。`claude_slash_commands` 對此沒有影響（已 probe）。
pub fn render_for_agent(body: &str, agent: &str, spec_dir: &str) -> anyhow::Result<String> {
    if !supported_agents().contains(&agent) {
        anyhow::bail!(
            "Unknown agent: {agent}. Supported agents: {}",
            supported_agents().join(", ")
        );
    }
    let spec_dir = format!("{}/", spec_dir.trim_end_matches('/'));
    let invocation = match agent {
        "claude" | "junie" => "/spectra-",
        "codex" => "$spectra-",
        _ => "/spectra:",
    };
    // ingest／propose 的 plan 目錄說明：只有 claude、cursor 有 plan 目錄，其餘 agent
    // 用固定的「沒有 plan 目錄」句子，`{{PLAN_DIR}}` 代入空字串（oracle 渲染反推）。
    let plan_dir = match agent {
        "claude" => Some("~/.claude/plans/"),
        "cursor" => Some(".cursor/plans/"),
        _ => None,
    };
    let (support_note, discovery_rule) = match plan_dir {
        Some(dir) => (
            format!("This tool can discover bare plan names under `{dir}` and also accepts explicit existing plan paths."),
            format!("Otherwise, if the bare argument resolves to a plan under `{dir}`, set `requirement_source` to that plan and select `target_change` separately."),
        ),
        None => (
            "This tool has no configured plan directory. It still accepts an explicit existing plan path; bare plan-name discovery is unavailable.".to_string(),
            "This tool has no configured plan directory, so skip bare plan-name lookup. An explicit existing plan path remains valid; otherwise continue only through the no-argument conversation flow.".to_string(),
        ),
    };
    Ok(body
        .replace("{{TOOL}}", agent)
        .replace("{{SPEC_DIR}}", &spec_dir)
        .replace("{{PLAN_SUPPORT_NOTE}}", &support_note)
        .replace("{{PLAN_DISCOVERY_RULE}}", &discovery_rule)
        .replace("{{PLAN_DIR}}", plan_dir.unwrap_or(""))
        .replace("/spectra:", invocation))
}

/// 所有內嵌 skill（registry 順序），供驗證 skill 內容的測試使用。
pub fn all_skills() -> &'static [(&'static str, &'static str)] {
    SKILLS
}

pub fn skill_body(name: &str) -> Option<&'static str> {
    SKILLS
        .iter()
        .find_map(|(skill, body)| (*skill == name).then_some(*body))
}

#[cfg(test)]
mod tests {
    use super::{skill_body, SKILLS};

    #[test]
    fn registry_contains_the_captured_oracle_skills() {
        let names = SKILLS.iter().map(|(name, _)| *name).collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "tdd",
                "audit",
                "apply",
                "archive",
                "commit",
                "debug",
                "discuss",
                "drift",
                "ingest",
                "propose",
                "analyze",
                "verify",
                "review",
                "sync",
                "clarify",
                "test-scope",
                "commit-archive",
                "ingest-plan-mapping",
                "ingest-context-mapping",
                "verify-spec-coverage",
            ]
        );
        assert!(SKILLS.iter().all(|(_, body)| !body.is_empty()));
        for (name, body) in SKILLS {
            assert_eq!(skill_body(name), Some(*body));
        }
    }

    #[test]
    fn agent_rendering_matches_the_oracle_per_agent_rules() {
        use super::render_for_agent;
        let body = "run `/spectra:verify x` then `spectra instructions --skill a --agent {{TOOL}}` in `{{SPEC_DIR}}changes/`";
        assert_eq!(
            render_for_agent(body, "claude", "docs/spectra").unwrap(),
            "run `/spectra-verify x` then `spectra instructions --skill a --agent claude` in `docs/spectra/changes/`"
        );
        assert_eq!(
            render_for_agent(body, "junie", "openspec").unwrap(),
            "run `/spectra-verify x` then `spectra instructions --skill a --agent junie` in `openspec/changes/`"
        );
        assert_eq!(
            render_for_agent(body, "codex", "a/b/c").unwrap(),
            "run `$spectra-verify x` then `spectra instructions --skill a --agent codex` in `a/b/c/changes/`"
        );
        for agent in ["antigravity", "cursor", "github-copilot"] {
            assert_eq!(
                render_for_agent(body, agent, "docs/x/").unwrap(),
                format!("run `/spectra:verify x` then `spectra instructions --skill a --agent {agent}` in `docs/x/changes/`")
            );
        }
    }

    #[test]
    fn agent_rendering_fills_plan_placeholders_per_agent() {
        use super::render_for_agent;
        let body = "{{PLAN_SUPPORT_NOTE}}|{{PLAN_DISCOVERY_RULE}}|[{{PLAN_DIR}}]";
        let claude = render_for_agent(body, "claude", "openspec").unwrap();
        assert!(
            claude.starts_with("This tool can discover bare plan names under `~/.claude/plans/`")
        );
        assert!(claude.ends_with("|[~/.claude/plans/]"));
        let cursor = render_for_agent(body, "cursor", "openspec").unwrap();
        assert!(cursor.contains("resolves to a plan under `.cursor/plans/`"));
        for agent in ["antigravity", "codex", "github-copilot", "junie"] {
            let out = render_for_agent(body, agent, "openspec").unwrap();
            assert!(out.starts_with("This tool has no configured plan directory. It still accepts"));
            assert!(out.ends_with("|[]"), "{agent}: {out}");
        }
        // 內嵌 body 渲染後不應殘留任何 placeholder。
        for (name, body) in super::SKILLS {
            for agent in super::supported_agents() {
                let out = render_for_agent(body, agent, "openspec").unwrap();
                assert!(!out.contains("{{"), "{name}/{agent} left a placeholder");
            }
        }
    }

    #[test]
    fn agent_rendering_rejects_an_unknown_agent_with_the_oracle_message() {
        let err = super::render_for_agent("x", "nope", "openspec").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unknown agent: nope. Supported agents: antigravity, claude, codex, cursor, github-copilot, junie"
        );
    }

    #[test]
    fn every_skill_referenced_by_an_embedded_asset_is_in_the_registry() {
        // 5 個 skill 曾因列舉詞表沒涵蓋而漏抓；body 之間的 `--skill <name>` 引用是
        // 可在 CI 驗證的最低限度閉包（不需要 oracle）。
        let mut texts: Vec<&str> = SKILLS.iter().map(|(_, body)| *body).collect();
        for tool in crate::update::registry() {
            texts.extend(tool.files.iter().map(|f| f.template));
        }
        let mut missing = std::collections::BTreeSet::new();
        for text in texts {
            for part in text.split("--skill ").skip(1) {
                let name: String = part
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                if !name.is_empty() && skill_body(&name).is_none() {
                    missing.insert(name);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "referenced but not embedded: {missing:?}"
        );
    }

    #[test]
    fn lookup_rejects_names_outside_the_registry() {
        assert_eq!(skill_body("bogus"), None);
        assert_eq!(skill_body("TDD"), None);
        assert_eq!(skill_body("ask"), None);
    }
}
