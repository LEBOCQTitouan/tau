//! Closest-match helper for "did you mean…?" suggestions on unknown
//! skill names.
//!
//! Standard Wagner-Fischer dynamic-programming Levenshtein distance,
//! O(n*m) over the two input strings. Threshold ≤ 2 distance is the
//! suggestion cutoff (tunable per call).

/// Find the candidate in `candidates` with the smallest edit
/// distance to `query`. Returns `Some(&candidate)` if the best
/// candidate is within `max_dist`; `None` otherwise.
///
/// On ties (multiple candidates at the same distance), returns the
/// first encountered — stable for sorted input.
pub fn closest_match<'a>(
    query: &str,
    candidates: &'a [String],
    max_dist: usize,
) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = distance(query, c);
        if d > max_dist {
            continue;
        }
        match best {
            None => best = Some((d, c.as_str())),
            Some((bd, _)) if d < bd => best = Some((d, c.as_str())),
            _ => {}
        }
    }
    best.map(|(_, s)| s)
}

/// Wagner-Fischer Levenshtein distance.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_itself() {
        let candidates = vec!["critic".to_string(), "proofread".to_string()];
        assert_eq!(closest_match("critic", &candidates, 2), Some("critic"));
    }

    #[test]
    fn single_typo_returns_closest() {
        // "kritic" → "critic" is 1 edit (k→c).
        let candidates = vec!["critic".to_string(), "proofread".to_string()];
        assert_eq!(closest_match("kritic", &candidates, 2), Some("critic"));
    }

    #[test]
    fn distance_beyond_threshold_returns_none() {
        let candidates = vec!["critic".to_string()];
        // "xyz123" is much further than 2 edits from "critic".
        assert_eq!(closest_match("xyz123", &candidates, 2), None);
    }
}
