mod common;

use common::{spectra, TempDir};

const EXPECTED_JSON: &str = r#"[
  {
    "artifactId": "proposal",
    "hasContent": true,
    "templateName": "proposal.md"
  },
  {
    "artifactId": "specs",
    "hasContent": true,
    "templateName": "spec.md"
  },
  {
    "artifactId": "design",
    "hasContent": true,
    "templateName": "design.md"
  },
  {
    "artifactId": "tasks",
    "hasContent": true,
    "templateName": "tasks.md"
  }
]
"#;

const EXPECTED_TEXT: &str = "Templates (spec-driven)\n  ✓ proposal → proposal.md\n  ✓ specs → spec.md\n  ✓ design → design.md\n  ✓ tasks → tasks.md\n";

#[test]
fn templates_matches_oracle_outside_a_project_in_both_modes() {
    let root = TempDir::new("templates-outside");
    let json = spectra()
        .args(["templates", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(json.status.success(), "{json:?}");
    assert_eq!(String::from_utf8(json.stdout).unwrap(), EXPECTED_JSON);
    assert!(json.stderr.is_empty());

    let text = spectra()
        .args(["templates", "--no-color"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(text.status.success(), "{text:?}");
    assert_eq!(String::from_utf8(text.stdout).unwrap(), EXPECTED_TEXT);
    assert!(text.stderr.is_empty());
}

#[test]
fn templates_unknown_schema_matches_oracle_error_channel_and_exit() {
    let root = TempDir::new("templates-bogus");
    let output = spectra()
        .args(["templates", "--schema", "bogus", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: Schema not found: Schema 'bogus' not found in project, user, or built-in locations\n"
    );
}
