//! `spectra demo`：以隨機名稱建立一個內含範例內容的 change（oracle 3.0.0，見
//! `docs/reverse-engineering/demo-feedback.md`）。
//!
//! 八個主題的檔案內容是 oracle 的逐位元組擷取（`assets/demo/`，由
//! `scripts/capture-demo.py` 產生並驗證，勿手改）；名稱是 `spx-<形容詞>-<寶可夢>`。

use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::change::ChangeMetadata;
use crate::config::Config;

/// oracle 3.0.0 的形容詞清單（600 次取樣觀察到的全部 20 個）。
pub const ADJECTIVES: [&str; 20] = [
    "bright", "eager", "gentle", "happy", "light", "proud", "quick", "sharp", "vivid", "calm",
    "dark", "fast", "keen", "neat", "rare", "tall", "warm", "bold", "cool", "deep",
];

/// oracle 3.0.0 的寶可夢清單（600 次取樣觀察到的全部 20 個）。
pub const POKEMON: [&str; 20] = [
    "pikachu",
    "charmander",
    "bulbasaur",
    "squirtle",
    "eevee",
    "snorlax",
    "gengar",
    "jigglypuff",
    "mewtwo",
    "dragonite",
    "lucario",
    "gardevoir",
    "charizard",
    "lapras",
    "umbreon",
    "absol",
    "arcanine",
    "gyarados",
    "rayquaza",
    "togekiss",
];

/// 一個示範主題：capability 名稱即主題名稱，`spec` 寫到 `specs/<name>/spec.md`。
pub struct Theme {
    pub name: &'static str,
    pub proposal: &'static str,
    pub design: &'static str,
    pub tasks: &'static str,
    pub spec: &'static str,
}

macro_rules! theme {
    ($name:literal) => {
        Theme {
            name: $name,
            proposal: include_str!(concat!("../assets/demo/", $name, "/proposal.md")),
            design: include_str!(concat!("../assets/demo/", $name, "/design.md")),
            tasks: include_str!(concat!("../assets/demo/", $name, "/tasks.md")),
            spec: include_str!(concat!(
                "../assets/demo/",
                $name,
                "/specs/",
                $name,
                "/spec.md"
            )),
        }
    };
}

/// oracle 3.0.0 的八個主題（依名稱排序；oracle 內部順序觀察不到）。
pub const THEMES: [Theme; 8] = [
    theme!("access-control"),
    theme!("audit-trail"),
    theme!("batch-export"),
    theme!("keyboard-macros"),
    theme!("real-time-sync"),
    theme!("smart-search"),
    theme!("snapshot-restore"),
    theme!("theme-engine"),
];

/// 名稱重複時最多嘗試的次數（oracle 字串：`after 20 attempts`）。
const NAME_ATTEMPTS: usize = 20;

/// 建立結果：人類輸出需要名稱、主題與絕對路徑。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoOutcome {
    pub name: String,
    pub theme: &'static str,
    pub path: PathBuf,
}

/// `0..n` 的隨機索引。只用於挑示範名稱與主題，不需要密碼學強度；以 std 的
/// `RandomState`（每個 process 隨機種子）雜湊一個遞增計數器，避免為此新增依賴。
fn random_index(n: usize) -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    (hasher.finish() % n as u64) as usize
}

/// 以系統亂數建立示範 change。
pub fn create(cfg: &Config) -> Result<DemoOutcome> {
    create_with(cfg, &mut random_index)
}

/// 以指定的索引來源建立示範 change（測試用可決定的來源）。
///
/// 依序挑形容詞、寶可夢，目錄已存在就重挑，最多 [`NAME_ATTEMPTS`] 次；再挑主題。
/// 只寫 change 目錄本身：`.openspec.yaml`（`schema` 一律 `spec-driven`，即使
/// config.yaml 指定別的 schema——oracle p18）、`proposal.md`、`design.md`、`tasks.md`、
/// `specs/<theme>/spec.md`；不寫任何 `.spectra/` 狀態（oracle p02）。
pub fn create_with(cfg: &Config, pick: &mut dyn FnMut(usize) -> usize) -> Result<DemoOutcome> {
    let changes_dir = cfg.changes_dir();
    let mut chosen = None;
    for _ in 0..NAME_ATTEMPTS {
        let name = format!(
            "spx-{}-{}",
            ADJECTIVES[pick(ADJECTIVES.len())],
            POKEMON[pick(POKEMON.len())]
        );
        if !changes_dir.join(&name).exists() {
            chosen = Some(name);
            break;
        }
    }
    let Some(name) = chosen else {
        anyhow::bail!("Could not generate a unique change name after {NAME_ATTEMPTS} attempts.");
    };
    let theme = &THEMES[pick(THEMES.len())];
    let dir = changes_dir.join(&name);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let metadata = ChangeMetadata {
        schema: Some(crate::schema::SCHEMA_NAME.to_string()),
        created: Some(chrono::Local::now().date_naive().to_string()),
        created_by: Some(crate::git::change_creator_identity(&cfg.root)),
        ..Default::default()
    };
    let yaml = serde_yaml::to_string(&metadata).context("serializing demo metadata")?;
    write(&dir.join(".openspec.yaml"), &yaml)?;
    write(&dir.join("proposal.md"), theme.proposal)?;
    write(&dir.join("design.md"), theme.design)?;
    write(&dir.join("tasks.md"), theme.tasks)?;
    let spec_dir = dir.join("specs").join(theme.name);
    std::fs::create_dir_all(&spec_dir)
        .with_context(|| format!("Failed to create specs directory: {}", spec_dir.display()))?;
    write(&spec_dir.join("spec.md"), theme.spec)?;
    Ok(DemoOutcome {
        name,
        theme: theme.name,
        path: dir,
    })
}

fn write(path: &std::path::Path, text: &str) -> Result<()> {
    std::fs::write(path, text).with_context(|| format!("Failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn project(dir: &std::path::Path) -> Config {
        std::fs::create_dir_all(dir.join("openspec/changes/archive")).unwrap();
        std::fs::create_dir_all(dir.join("openspec/specs")).unwrap();
        Config {
            root: dir.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    #[test]
    fn writes_exactly_the_oracle_file_set_for_the_picked_theme() {
        let dir = TempDir::new("demo-files");
        let cfg = project(&dir);
        // 索引依序：形容詞 16（warm）、寶可夢 10（lucario）、主題 2（batch-export）。
        let mut picks = [16, 10, 2].into_iter();
        let outcome = create_with(&cfg, &mut |_| picks.next().unwrap()).unwrap();
        assert_eq!(outcome.name, "spx-warm-lucario");
        assert_eq!(outcome.theme, "batch-export");
        assert_eq!(outcome.path, dir.join("openspec/changes/spx-warm-lucario"));
        let mut files: Vec<String> = Vec::new();
        let mut stack = vec![outcome.path.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    files.push(
                        e.path()
                            .strip_prefix(&outcome.path)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
        }
        files.sort();
        assert_eq!(
            files,
            vec![
                ".openspec.yaml",
                "design.md",
                "proposal.md",
                "specs/batch-export/spec.md",
                "tasks.md"
            ]
        );
        let meta = std::fs::read_to_string(outcome.path.join(".openspec.yaml")).unwrap();
        assert!(meta.starts_with("schema: spec-driven\ncreated: "), "{meta}");
        assert!(meta.contains("\ncreated_by: "), "{meta}");
        // oracle 3.0.0 p02 擷取到的 batch-export proposal 開頭。
        assert!(std::fs::read_to_string(outcome.path.join("proposal.md"))
            .unwrap()
            .starts_with("## Why\n\nUsers need to export multiple items at once"));
        assert!(!dir.join(".spectra").exists());
    }

    #[test]
    fn schema_is_always_spec_driven_even_with_a_custom_project_schema() {
        let dir = TempDir::new("demo-schema");
        let cfg = project(&dir);
        std::fs::write(dir.join("openspec/config.yaml"), "schema: custom-x\n").unwrap();
        let outcome = create_with(&cfg, &mut |_| 0).unwrap();
        let meta = std::fs::read_to_string(outcome.path.join(".openspec.yaml")).unwrap();
        assert!(meta.starts_with("schema: spec-driven\n"), "{meta}");
    }

    #[test]
    fn retries_taken_names_and_gives_up_after_twenty_attempts() {
        let dir = TempDir::new("demo-collide");
        let cfg = project(&dir);
        std::fs::create_dir_all(dir.join("openspec/changes/spx-bright-pikachu")).unwrap();
        // 第一次挑到已存在的名稱，第二次挑到 (1, 1)。
        let mut picks = [0, 0, 1, 1, 0].into_iter();
        let outcome = create_with(&cfg, &mut |_| picks.next().unwrap()).unwrap();
        assert_eq!(outcome.name, "spx-eager-charmander");

        let err = create_with(&cfg, &mut |_| 0).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Could not generate a unique change name after 20 attempts."
        );
    }

    #[test]
    fn random_index_stays_in_range_and_varies() {
        let seen: std::collections::BTreeSet<usize> = (0..200).map(|_| random_index(8)).collect();
        assert!(seen.iter().all(|&i| i < 8));
        assert!(seen.len() > 1);
    }

    #[test]
    fn every_theme_writes_its_own_capability() {
        for theme in &THEMES {
            assert!(
                theme.proposal.contains(&format!("- `{}`: ", theme.name)),
                "{}",
                theme.name
            );
            assert!(theme.spec.starts_with("## ADDED Requirements\n"));
        }
    }
}
