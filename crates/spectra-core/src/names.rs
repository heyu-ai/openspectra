//! Shared name-validation guard for `change`/`spec` lookups.

/// `name` must be a single path component (no separators or `..`) — CLI
/// commands (e.g. `spectra show`) pass raw user input straight through, so
/// this guard is load-bearing, not defensive-for-a-hypothetical-caller.
pub(crate) fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
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
    }

    #[test]
    fn accepts_ordinary_names() {
        assert!(is_valid_name("auth"));
        assert!(is_valid_name("my-change"));
    }
}
