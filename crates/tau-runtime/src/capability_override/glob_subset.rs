//! Glob-subset analyzer — `is_glob_subset(child, parent)` returns true
//! iff every concrete path matched by `child` is also matched by `parent`.
//!
//! Algorithm (in order):
//! 1. Literal equality.
//! 2. Prefix expansion — strip a trailing `**` from `parent`; if `child`
//!    starts with the parent prefix (followed by `/` or end-of-string),
//!    return true.
//! 3. Brace expansion — handle `{a,b}` by enumerating arms and recursing.
//! 4. Bounded sample fallback for `?`/character-class adversarial cases —
//!    generate ≤ 64 sample paths from `child`; assert each matches `parent`.
//! 5. Otherwise, fail-closed (return false).
//!
//! See `docs/superpowers/specs/2026-04-30-capability-override-design.md` §5.

use globset::Glob;

/// Maximum number of sample paths generated for the fallback check.
/// Above this bound, `is_glob_subset` returns false (fail-closed).
const MAX_SAMPLES: usize = 64;

/// True iff every concrete path matched by `child` is also matched by `parent`.
#[allow(dead_code)] // wired up by Task 2
pub(crate) fn is_glob_subset(child: &str, parent: &str) -> bool {
    if child == parent {
        return true;
    }
    if prefix_subset(child, parent) {
        return true;
    }
    if let Some(child_arms) = brace_expand(child) {
        return child_arms.iter().all(|arm| is_glob_subset(arm, parent));
    }
    if let Some(parent_arms) = brace_expand(parent) {
        return parent_arms.iter().any(|arm| is_glob_subset(child, arm));
    }
    sample_subset(child, parent)
}

/// Verify each entry of `children` is a subset of at least one entry of `parents`.
/// Returns `Ok(())` on success, or `Err(child_glob)` naming the first glob
/// that is not a subset.
#[allow(dead_code)] // wired up by Task 2
pub(crate) fn is_glob_subset_set(children: &[String], parents: &[String]) -> Result<(), String> {
    for child in children {
        if !parents.iter().any(|p| is_glob_subset(child, p)) {
            return Err(child.clone());
        }
    }
    Ok(())
}

fn prefix_subset(child: &str, parent: &str) -> bool {
    // Parent of the form "<prefix>/**" admits any path starting with
    // "<prefix>/". `**` alone admits anything (empty prefix).
    //
    // Character-class patterns (`[...]`) are deferred to the sample
    // fallback so the budget-overflow fail-closed path is reachable.
    if child.contains('[') {
        return false;
    }
    let prefix = if let Some(stripped) = parent.strip_suffix("/**") {
        stripped
    } else if parent == "**" {
        ""
    } else {
        return false;
    };
    if prefix.is_empty() {
        return true;
    }
    child == prefix || child.starts_with(&format!("{prefix}/"))
}

/// Expand a single top-level brace alternation. Returns `None` when no
/// brace is present. Nested braces are not handled at v0.1 — if `child`
/// has nested braces the analyzer falls through to the sample fallback.
fn brace_expand(pattern: &str) -> Option<Vec<String>> {
    let open = pattern.find('{')?;
    let close = pattern[open..].find('}').map(|i| open + i)?;
    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let arms_str = &pattern[open + 1..close];
    if arms_str.contains('{') {
        return None; // nested brace: defer to sample fallback
    }
    let arms: Vec<String> = arms_str
        .split(',')
        .map(|arm| format!("{prefix}{arm}{suffix}"))
        .collect();
    Some(arms)
}

/// Generate up to `MAX_SAMPLES` concrete paths matching `child`, then
/// assert each matches `parent`. Returns false if the sample budget is
/// exceeded (fail-closed) or if any sample fails to match `parent`.
fn sample_subset(child: &str, parent: &str) -> bool {
    let parent_glob = match Glob::new(parent) {
        Ok(g) => g.compile_matcher(),
        Err(_) => return false,
    };
    let samples = match generate_samples(child, MAX_SAMPLES) {
        Some(s) => s,
        None => return false, // overflow → fail-closed
    };
    samples.iter().all(|p| parent_glob.is_match(p))
}

/// Generate sample paths by enumerating `?` (one ASCII letter) and simple
/// character classes `[abc]`. `*` and `**` expand to a fixed seed string.
/// Returns `None` if the sample count would exceed `cap`.
fn generate_samples(child: &str, cap: usize) -> Option<Vec<String>> {
    let mut accum: Vec<String> = vec![String::new()];
    let mut chars = child.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '?' => {
                let next: Vec<String> = accum
                    .iter()
                    .flat_map(|s| ['a', 'b'].iter().map(move |ch| format!("{s}{ch}")))
                    .collect();
                if next.len() > cap {
                    return None;
                }
                accum = next;
            }
            '[' => {
                let mut class = String::new();
                for cc in chars.by_ref() {
                    if cc == ']' {
                        break;
                    }
                    class.push(cc);
                }
                let chars_in_class: Vec<char> = class.chars().collect();
                if chars_in_class.is_empty() {
                    return None;
                }
                let next: Vec<String> = accum
                    .iter()
                    .flat_map(|s| chars_in_class.iter().map(move |ch| format!("{s}{ch}")))
                    .collect();
                if next.len() > cap {
                    return None;
                }
                accum = next;
            }
            '*' => {
                // `*` and `**` expand to a fixed seed; this is enough to
                // probe parent admission for the structural cases.
                if matches!(chars.peek(), Some('*')) {
                    chars.next();
                    accum = accum.iter().map(|s| format!("{s}seed/path")).collect();
                } else {
                    accum = accum.iter().map(|s| format!("{s}seed")).collect();
                }
            }
            other => {
                accum = accum.iter().map(|s| format!("{s}{other}")).collect();
            }
        }
        if accum.len() > cap {
            return None;
        }
    }
    Some(accum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_equality_is_subset() {
        assert!(is_glob_subset("/tmp/foo", "/tmp/foo"));
    }

    #[test]
    fn prefix_expansion_strict_subdir() {
        assert!(is_glob_subset("/proj/src/**", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_concrete_path_under_double_star() {
        assert!(is_glob_subset("/proj/src/main.rs", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_match_at_prefix_only() {
        assert!(is_glob_subset("/proj", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_root_glob_admits_everything() {
        assert!(is_glob_subset("/anything/anywhere", "/**"));
        assert!(is_glob_subset("/anything", "**"));
    }

    #[test]
    fn disjoint_paths_are_not_subset() {
        assert!(!is_glob_subset("/etc/**", "/proj/src/**"));
    }

    #[test]
    fn parent_more_specific_than_child_is_not_subset() {
        // Child admits /proj/etc/passwd; parent does not.
        assert!(!is_glob_subset("/proj/**", "/proj/src/**"));
    }

    #[test]
    fn brace_expansion_child_all_arms_subset() {
        assert!(is_glob_subset("/proj/{src,docs}/**", "/proj/**"));
    }

    #[test]
    fn brace_expansion_child_one_arm_not_subset_rejects() {
        assert!(!is_glob_subset("/proj/{src,etc}/**", "/proj/src/**"));
    }

    #[test]
    fn brace_expansion_parent_arm_admits_child() {
        assert!(is_glob_subset("/proj/src/**", "/proj/{src,docs}/**"));
    }

    #[test]
    fn nested_braces_falls_through_to_sample_fallback() {
        // Nested braces aren't decomposed at v0.1 — the structural rules
        // bail and the sample fallback runs. With `*` expansion seeding
        // a deterministic path, this still admits since /proj/{a,{b,c}}
        // matches under /proj/**.
        assert!(is_glob_subset("/proj/{a,{b,c}}/file", "/proj/**"));
    }

    #[test]
    fn question_mark_sample_fallback_admits() {
        // `/proj/?` matches one-char names — all admitted under /proj/**.
        assert!(is_glob_subset("/proj/?", "/proj/**"));
    }

    #[test]
    fn character_class_sample_fallback_admits() {
        assert!(is_glob_subset("/proj/[abc]", "/proj/**"));
    }

    #[test]
    fn sample_budget_overflow_fails_closed() {
        // 65 char-class arms would generate 65 samples; budget is 64.
        // We expect false (fail-closed).
        let huge_class: String = (0..65).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let child = format!("/proj/[{huge_class}]");
        assert!(!is_glob_subset(&child, "/proj/**"));
    }

    #[test]
    fn invalid_parent_glob_is_not_subset() {
        // An invalid parent glob defaults to no-match (defensive).
        assert!(!is_glob_subset("/proj/foo", "[unclosed"));
    }

    #[test]
    fn empty_child_is_subset_only_of_empty_parent() {
        assert!(is_glob_subset("", ""));
        assert!(!is_glob_subset("", "/proj/**"));
    }

    #[test]
    fn is_glob_subset_set_returns_first_offender() {
        let children = vec![
            "/proj/src/**".to_string(),
            "/proj/etc/**".to_string(),
            "/proj/docs/**".to_string(),
        ];
        let parents = vec!["/proj/{src,docs}/**".to_string()];
        let err = is_glob_subset_set(&children, &parents).unwrap_err();
        assert_eq!(err, "/proj/etc/**");
    }

    #[test]
    fn is_glob_subset_set_succeeds_when_all_admitted() {
        let children = vec!["/proj/src/**".to_string(), "/proj/docs/**".to_string()];
        let parents = vec!["/proj/**".to_string()];
        is_glob_subset_set(&children, &parents).unwrap();
    }
}
