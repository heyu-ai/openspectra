//! Shared name-validation guard for `change`/`spec` lookups.

/// `name` must be a single path component (no separators, `..`, or a
/// Windows drive prefix like `C:`) — CLI commands (e.g. `spectra show`) pass
/// raw user input straight through, so this guard is load-bearing, not
/// defensive-for-a-hypothetical-caller.
pub(crate) fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && name != "."
        && name != ".."
}

/// Oracle-compatible kebab-case predicate used for active change and
/// capability names. Consecutive hyphens are accepted, but leading/trailing
/// hyphens and non-ASCII lowercase/digit characters are not.
pub(crate) fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_empty_names() {
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("."));
        assert!(!is_valid_name(".."));
        assert!(!is_valid_name("../secret"));
        assert!(!is_valid_name("sub/dir"));
        assert!(!is_valid_name("sub\\dir"));
        assert!(!is_valid_name("C:secret"));
    }

    #[test]
    fn accepts_ordinary_names() {
        assert!(is_valid_name("auth"));
        assert!(is_valid_name("my-change"));
    }

    #[test]
    fn kebab_case_matches_change_and_capability_rules() {
        for valid in ["user-auth", "a--b", "cap2-v3", "a", "1"] {
            assert!(is_kebab_case(valid), "expected {valid:?} to be valid");
        }
        for invalid in ["", "Bad_Name", "bad-", "-bad", "has space"] {
            assert!(
                !is_kebab_case(invalid),
                "expected {invalid:?} to be invalid"
            );
        }
    }
}
