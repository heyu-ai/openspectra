//! Dependency-free lexical search over a project's Markdown artifacts.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::config::Config;

const K1: f64 = 1.2;
const B: f64 = 0.75;
const SNIPPET_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub path: String,
    pub score: f64,
    pub snippets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
}

#[derive(Debug)]
struct Document {
    path: PathBuf,
    content: String,
    terms: HashMap<String, usize>,
    len: usize,
}

/// Search every Markdown artifact below the configured spec directory.
pub fn search(cfg: &Config, query: &str, limit: usize) -> Result<SearchResponse> {
    let query_terms = term_counts(query);
    if limit == 0 || query_terms.is_empty() {
        return Ok(SearchResponse {
            query: query.to_string(),
            results: Vec::new(),
        });
    }

    let spec_dir = Path::new(&cfg.spec_dir);
    if spec_dir.is_absolute()
        || spec_dir.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("spec_dir must be a relative path within the project root");
    }

    let mut paths = Vec::new();
    collect_markdown(&cfg.root.join(spec_dir), &mut paths)?;
    paths.sort();

    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading search document {}", path.display()))?;
        let terms = term_counts(&content);
        let len = terms.values().sum();
        documents.push(Document {
            path,
            content,
            terms,
            len,
        });
    }

    if documents.is_empty() {
        return Ok(SearchResponse {
            query: query.to_string(),
            results: Vec::new(),
        });
    }

    let average_len =
        documents.iter().map(|doc| doc.len).sum::<usize>() as f64 / documents.len() as f64;
    let document_count = documents.len() as f64;
    let document_frequency = query_terms
        .keys()
        .map(|term| {
            let count = documents
                .iter()
                .filter(|doc| doc.terms.contains_key(term))
                .count();
            (term, count as f64)
        })
        .collect::<HashMap<_, _>>();

    let mut ranked = documents
        .into_iter()
        .filter_map(|doc| {
            let mut raw_score = 0.0;
            for (term, query_frequency) in &query_terms {
                let frequency = *doc.terms.get(term).unwrap_or(&0) as f64;
                if frequency == 0.0 {
                    continue;
                }
                let df = document_frequency[term];
                let inverse_document_frequency =
                    (1.0 + (document_count - df + 0.5) / (df + 0.5)).ln();
                let length_ratio = if average_len == 0.0 {
                    0.0
                } else {
                    doc.len as f64 / average_len
                };
                let saturation =
                    frequency * (K1 + 1.0) / (frequency + K1 * (1.0 - B + B * length_ratio));
                raw_score += inverse_document_frequency
                    * saturation
                    * (1.0 + (*query_frequency as f64).ln());
            }
            (raw_score > 0.0).then(|| {
                let path = doc
                    .path
                    .strip_prefix(&cfg.root)
                    .unwrap_or(&doc.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                SearchResult {
                    path,
                    score: raw_score / (raw_score + 1.0),
                    snippets: vec![snippet(&doc.content, query_terms.keys())],
                }
            })
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    ranked.truncate(limit);

    Ok(SearchResponse {
        query: query.to_string(),
        results: ranked,
    })
}

fn collect_markdown(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for {}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_markdown(&entry.path(), output)?;
        } else if file_type.is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("md")
        {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn term_counts(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for term in tokenize(text) {
        *counts.entry(term).or_insert(0) += 1;
    }
    counts
}

fn tokenize(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut word = String::new();
    let mut cjk_run = Vec::new();

    let flush_word = |word: &mut String, terms: &mut Vec<String>| {
        if !word.is_empty() {
            terms.push(std::mem::take(word));
        }
    };
    let flush_cjk = |run: &mut Vec<char>, terms: &mut Vec<String>| {
        for character in run.iter() {
            terms.push(character.to_string());
        }
        for pair in run.windows(2) {
            terms.push(pair.iter().collect());
        }
        run.clear();
    };

    for character in text.chars().flat_map(char::to_lowercase) {
        if is_cjk(character) {
            flush_word(&mut word, &mut terms);
            cjk_run.push(character);
        } else if character.is_alphanumeric() || character == '_' {
            flush_cjk(&mut cjk_run, &mut terms);
            word.push(character);
        } else {
            flush_word(&mut word, &mut terms);
            flush_cjk(&mut cjk_run, &mut terms);
        }
    }
    flush_word(&mut word, &mut terms);
    flush_cjk(&mut cjk_run, &mut terms);
    terms
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
    )
}

fn snippet<'a>(content: &str, query_terms: impl Iterator<Item = &'a String>) -> String {
    let needles = query_terms.collect::<HashSet<_>>();
    let mut lowered = String::new();
    let mut source_positions = Vec::new();
    for (source_index, character) in content.chars().enumerate() {
        for lowered_character in character.to_lowercase() {
            source_positions.push((lowered.len(), source_index));
            lowered.push(lowered_character);
        }
    }
    let match_char = needles
        .iter()
        .filter_map(|term| lowered.find(term.as_str()))
        .filter_map(|byte_index| {
            let position = source_positions
                .partition_point(|(lowered_byte, _)| *lowered_byte <= byte_index)
                .checked_sub(1)?;
            Some(source_positions[position].1)
        })
        .min()
        .unwrap_or(0);
    let characters = content.chars().collect::<Vec<_>>();
    let start = match_char.saturating_sub(SNIPPET_CHARS / 4);
    let end = (start + SNIPPET_CHARS).min(characters.len());
    let mut text = characters[start..end]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if start > 0 {
        text.insert(0, '…');
    }
    if end < characters.len() {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn config(root: &Path) -> Config {
        Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    #[test]
    fn ranks_specific_document_above_common_match_and_normalizes_score() {
        let root = TempDir::new("search-ranking");
        let specs = root.join("openspec/specs");
        std::fs::create_dir_all(specs.join("auth")).unwrap();
        std::fs::create_dir_all(specs.join("other")).unwrap();
        std::fs::write(specs.join("auth/spec.md"), "token token rotation policy").unwrap();
        std::fs::write(specs.join("other/spec.md"), "general token notes").unwrap();

        let response = search(&config(&root), "token rotation", 10).unwrap();
        assert_eq!(response.results[0].path, "openspec/specs/auth/spec.md");
        assert!(response.results.iter().all(|item| item.score > 0.0));
        assert!(response.results.iter().all(|item| item.score < 1.0));
    }

    #[test]
    fn cjk_bigrams_distinguish_phrases() {
        let terms = tokenize("封存變更");
        assert!(terms.contains(&"封存".to_string()));
        assert!(terms.contains(&"存變".to_string()));
        assert!(terms.contains(&"變更".to_string()));
    }

    #[test]
    fn zero_limit_and_empty_corpus_return_no_results() {
        let root = TempDir::new("search-empty");
        let cfg = config(&root);
        assert!(search(&cfg, "anything", 10).unwrap().results.is_empty());
        assert!(search(&cfg, "anything", 0).unwrap().results.is_empty());
    }

    #[test]
    fn snippet_handles_lowercase_expansion_before_a_match() {
        let content = format!("{} needle at the end", "İ".repeat(100));
        let terms = term_counts("needle");
        let result = snippet(&content, terms.keys());
        assert!(result.contains("needle"));
    }

    #[test]
    fn rejects_spec_directories_that_escape_the_project_root() {
        let root = TempDir::new("search-spec-dir-escape");
        for spec_dir in ["../outside", "/tmp/outside"] {
            let mut cfg = config(&root);
            cfg.spec_dir = spec_dir.to_string();
            let error = search(&cfg, "secret", 10).unwrap_err();
            assert_eq!(
                error.to_string(),
                "spec_dir must be a relative path within the project root"
            );
        }
    }

    #[test]
    fn scans_active_and_archived_changes_as_well_as_specs() {
        let root = TempDir::new("search-corpus");
        for relative in [
            "openspec/specs/cap/spec.md",
            "openspec/changes/current/proposal.md",
            "openspec/changes/archive/2026-01-01-old/design.md",
        ] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("unique {relative}")).unwrap();
        }
        let response = search(&config(&root), "unique", 10).unwrap();
        assert_eq!(response.results.len(), 3);
    }
}
