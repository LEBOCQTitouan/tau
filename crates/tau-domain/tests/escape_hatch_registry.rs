//! Mechanical enforcement of the escape-hatch registry rule from ADR-0002.
//!
//! Walks every `.rs` file in `crates/`. For each variant named `Custom`
//! or `InternalError`, requires its preceding rustdoc to contain a link
//! to `escape-hatches.md#<anchor>`. Verifies the registry file contains
//! a matching anchor for each. Stale anchors (in registry but not in
//! source) also fail the test.

use std::collections::HashSet;
use std::path::PathBuf;

use walkdir::WalkDir;

const REGISTRY_PATH: &str = "../../docs/explanation/escape-hatches.md";
const CRATES_ROOT: &str = "../../crates";
const ESCAPE_HATCH_VARIANTS: &[&str] = &["Custom", "InternalError"];

#[derive(Debug)]
struct SourceHatch {
    file: PathBuf,
    line: usize,
    variant: String,
    anchor: Option<String>,
}

fn parse_registry_anchors() -> HashSet<String> {
    let raw = std::fs::read_to_string(REGISTRY_PATH).expect("registry file must exist");
    let mut found = HashSet::new();
    // Look for `<a id="anchor-name"></a>` patterns inside the active
    // table.
    let mut in_active_section = false;
    for line in raw.lines() {
        let lt = line.trim();
        if lt.starts_with("## Active escape hatches") {
            in_active_section = true;
            continue;
        }
        if in_active_section && lt.starts_with("## ") {
            break;
        }
        if !in_active_section {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find(r#"<a id=""#) {
            let after = &rest[start + r#"<a id=""#.len()..];
            if let Some(end) = after.find('"') {
                let anchor = &after[..end];
                found.insert(anchor.to_string());
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
    }
    found
}

fn find_escape_hatches() -> Vec<SourceHatch> {
    let mut hatches = Vec::new();
    for entry in WalkDir::new(CRATES_ROOT).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let lines: Vec<&str> = raw.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            for variant in ESCAPE_HATCH_VARIANTS {
                let after_keyword = trimmed
                    .strip_prefix(variant)
                    .or_else(|| trimmed.strip_prefix(&format!("#[non_exhaustive] {variant}")));
                if let Some(rest) = after_keyword {
                    let next_char = rest.chars().next();
                    if matches!(
                        next_char,
                        Some(' ') | Some('{') | Some('(') | Some(',') | None
                    ) {
                        // Look back through immediately-preceding doc comments.
                        let mut anchor: Option<String> = None;
                        let mut j = i;
                        while j > 0 {
                            j -= 1;
                            let prev = lines[j].trim();
                            if prev.starts_with("///") || prev.starts_with("//!") || prev.is_empty()
                            {
                                if let Some(start) = prev.find("escape-hatches.md#") {
                                    let after = &prev[start + "escape-hatches.md#".len()..];
                                    let end = after
                                        .find(|c: char| {
                                            c == ')' || c == ']' || c == ' ' || c == '"'
                                        })
                                        .unwrap_or(after.len());
                                    anchor = Some(after[..end].to_string());
                                    break;
                                }
                                if !prev.starts_with("///") && !prev.starts_with("//!") {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        hatches.push(SourceHatch {
                            file: path.to_owned(),
                            line: i + 1,
                            variant: (*variant).to_string(),
                            anchor,
                        });
                    }
                }
            }
        }
    }
    hatches
}

#[test]
fn every_escape_hatch_is_registered() {
    let registered = parse_registry_anchors();
    let source = find_escape_hatches();

    assert!(
        !source.is_empty(),
        "no escape hatches found in source — test scanner is probably broken",
    );

    let mut missing = Vec::new();
    let mut live_anchors: HashSet<String> = HashSet::new();
    for h in &source {
        match &h.anchor {
            None => missing.push(format!(
                "{}:{} variant `{}` has no rustdoc link to escape-hatches.md",
                h.file.display(),
                h.line,
                h.variant,
            )),
            Some(a) if !registered.contains(a) => missing.push(format!(
                "{}:{} variant `{}` references unknown anchor `{}`",
                h.file.display(),
                h.line,
                h.variant,
                a,
            )),
            Some(a) => {
                live_anchors.insert(a.clone());
            }
        }
    }

    let stale: Vec<_> = registered.difference(&live_anchors).collect();

    let mut errs = missing;
    for s in stale {
        errs.push(format!(
            "registry anchor `{s}` is not used by any source variant (stale entry)"
        ));
    }

    assert!(
        errs.is_empty(),
        "escape-hatch registry mismatches:\n{}",
        errs.join("\n")
    );
}
