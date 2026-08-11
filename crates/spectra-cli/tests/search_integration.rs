mod common;

use common::{spectra, TempDir};

#[test]
fn search_json_has_consumer_contract_and_honors_limit() {
    let root = TempDir::new("search-json");
    let init = spectra().arg("init").current_dir(&*root).output().unwrap();
    assert!(init.status.success());
    let spec = root.join("openspec/specs/session/spec.md");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(&spec, "Session tokens rotate after authentication.").unwrap();

    let output = spectra()
        .args(["search", "authentication token", "--limit", "1", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["query"], "authentication token");
    assert_eq!(value["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["results"][0]["path"],
        "openspec/specs/session/spec.md"
    );
    assert!(value["results"][0]["score"].is_number());
    assert!(value["results"][0]["snippets"].is_array());
    assert!(value.get("error").is_none());
}

#[test]
fn search_requires_init_and_empty_project_is_successful() {
    let outside = TempDir::new("search-outside");
    let failure = spectra()
        .args(["search", "needle", "--json"])
        .current_dir(&*outside)
        .output()
        .unwrap();
    assert_eq!(failure.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failure.stderr).contains("Not initialized"));

    let root = TempDir::new("search-empty");
    assert!(spectra()
        .arg("init")
        .current_dir(&*root)
        .status()
        .unwrap()
        .success());
    let output = spectra()
        .args(["search", "needle", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"query\":\"needle\",\"results\":[]}\n"
    );
}

#[test]
fn text_mode_matches_observable_oracle_messages() {
    let root = TempDir::new("search-text");
    assert!(spectra()
        .arg("init")
        .current_dir(&*root)
        .status()
        .unwrap()
        .success());
    let empty = spectra()
        .args(["search", "needle"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(empty.stdout).unwrap(),
        "No results found.\n"
    );
}
