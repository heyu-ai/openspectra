//! 內嵌內容是從 Spectra 2.3.1 oracle 逐位元擷取的 skill 本文。
//! Registry 的項目與順序由 `scripts/capture-skills.py` 交叉檢查。
//! 靜態資產與 `docs/reverse-engineering/golden/skills-2.3.1.tsv` 都是產生物，請勿手動編輯。

const SKILLS: &[(&str, &str)] = &[
    ("tdd", include_str!("../assets/skills/tdd.md")),
    ("audit", include_str!("../assets/skills/audit.md")),
    ("apply", include_str!("../assets/skills/apply.md")),
    ("archive", include_str!("../assets/skills/archive.md")),
    ("ask", include_str!("../assets/skills/ask.md")),
    ("commit", include_str!("../assets/skills/commit.md")),
    ("debug", include_str!("../assets/skills/debug.md")),
    ("discuss", include_str!("../assets/skills/discuss.md")),
    ("drift", include_str!("../assets/skills/drift.md")),
    ("ingest", include_str!("../assets/skills/ingest.md")),
    ("propose", include_str!("../assets/skills/propose.md")),
    ("analyze", include_str!("../assets/skills/analyze.md")),
    ("verify", include_str!("../assets/skills/verify.md")),
    ("sync", include_str!("../assets/skills/sync.md")),
    ("clarify", include_str!("../assets/skills/clarify.md")),
];

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
                "tdd", "audit", "apply", "archive", "ask", "commit", "debug", "discuss", "drift",
                "ingest", "propose", "analyze", "verify", "sync", "clarify",
            ]
        );
        assert!(SKILLS.iter().all(|(_, body)| !body.is_empty()));
        for (name, body) in SKILLS {
            assert_eq!(skill_body(name), Some(*body));
        }
    }

    #[test]
    fn lookup_rejects_names_outside_the_registry() {
        assert_eq!(skill_body("bogus"), None);
        assert_eq!(skill_body("TDD"), None);
    }
}
