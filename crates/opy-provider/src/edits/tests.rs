use super::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn syntax_error_classification_is_exhaustive() {
    let mut found = BTreeSet::new();
    collect_literals(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../opy-rs/src"),
        &mut found,
    );

    let classified: Vec<&str> = CODE_CLASSES
        .iter()
        .flat_map(|(_, codes)| codes.iter().copied())
        .collect();
    let listed: BTreeSet<&str> = classified.iter().copied().collect();
    assert_eq!(
        classified.len(),
        listed.len(),
        "a literal is classified under more than one class"
    );
    let non_code: BTreeSet<&str> = NON_DIAGNOSTIC_LITERALS.iter().copied().collect();
    assert!(
        listed.is_disjoint(&non_code),
        "a literal appears in both the classification table and the non-code list"
    );

    let unclassified: Vec<&String> = found
        .iter()
        .filter(|code| !listed.contains(code.as_str()) && !non_code.contains(code.as_str()))
        .collect();
    let stale: Vec<&str> = listed
        .iter()
        .chain(non_code.iter())
        .copied()
        .filter(|code| !found.contains(*code))
        .collect();
    assert!(
        unclassified.is_empty() && stale.is_empty(),
        "unclassified literals: {unclassified:?}; listed but never emitted: {stale:?}"
    );
}

fn collect_literals(dir: &Path, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).expect("read opy-rs sources") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if name != "tests" {
                collect_literals(&path, found);
            }
        } else if name.ends_with(".rs") && !name.starts_with("test_") && name != "tests.rs" {
            let source = fs::read_to_string(&path).expect("read source file");
            let source = source.split("#[cfg(test)]").next().unwrap_or("");
            scan(source, found);
        }
    }
}

fn scan(source: &str, found: &mut BTreeSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = bytes[i..]
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|end| i + end)
                    .unwrap_or(bytes.len());
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'r' => {
                let mut hashes = 0;
                while bytes.get(i + 1 + hashes) == Some(&b'#') {
                    hashes += 1;
                }
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let mut j = i + 2 + hashes;
                    while j < bytes.len()
                        && !(bytes[j] == b'"'
                            && (0..hashes).all(|h| bytes.get(j + 1 + h) == Some(&b'#')))
                    {
                        j += 1;
                    }
                    i = j + 1 + hashes;
                } else {
                    i += 1;
                }
            }
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += usize::from(bytes[j] == b'\\') + 1;
                }
                let text = &source[start..j.min(bytes.len())];
                if is_code_shape(text) {
                    found.insert(text.to_string());
                }
                i = j + 1;
            }
            b'\'' => {
                let next = bytes.get(i + 1).copied();
                let after = if next == Some(b'\\') {
                    (i + 2..bytes.len().min(i + 8))
                        .find(|&j| bytes[j] == b'\'')
                        .map(|j| j + 1)
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    Some(i + 3)
                } else {
                    None
                };
                i = after.unwrap_or(i + 1);
            }
            _ => i += 1,
        }
    }
}

fn is_code_shape(text: &str) -> bool {
    text.contains('-')
        && text.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
