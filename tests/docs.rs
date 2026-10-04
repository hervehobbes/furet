// WHY: this whole file is layer-3 test code, where expect() is the norm.
#![allow(clippy::expect_used)]

use regex::Regex;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const DOCUMENTS: [&str; 4] = [
    "CONTRACTS.md",
    "DATABASE.md",
    "ARCHITECTURE.md",
    "README.md",
];

fn collect_rs_files(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let entries = fs::read_dir(dir).expect("a readable directory");
    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_rs_files(&path, root, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let relative = path
                .strip_prefix(root)
                .expect("a path under the crate root");
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn code_corpus(root: &Path) -> BTreeMap<String, String> {
    let mut paths = Vec::new();
    collect_rs_files(&root.join("src"), root, &mut paths);
    collect_rs_files(&root.join("tests"), root, &mut paths);
    paths.push("build.rs".to_string());
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|relative| {
            let text = fs::read_to_string(root.join(&relative)).expect("a readable source file");
            (relative, text)
        })
        .collect()
}

fn read_documents(root: &Path) -> Vec<(String, String)> {
    DOCUMENTS
        .iter()
        .map(|name| {
            let text = fs::read_to_string(root.join(name)).expect("a readable document");
            (name.to_string(), text)
        })
        .collect()
}

fn whole_word_regex(word: &str) -> Regex {
    Regex::new(&format!(r"\b{}\b", regex::escape(word))).expect("a valid whole-word regex")
}

fn check_documents(root: &Path) -> (Vec<String>, usize) {
    let corpus = code_corpus(root);
    let mut code = String::new();
    for text in corpus.values() {
        code.push_str(text);
        code.push('\n');
    }
    let token_re = Regex::new(r"`([^`\n]+)`").expect("a valid token regex");
    let rule_a_re = Regex::new(r"^[a-z][a-z0-9]*(_[a-z0-9]+){3,}$").expect("a valid rule A regex");
    let rule_b_re = Regex::new(r"^([a-z_][a-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)(\(\))?$")
        .expect("a valid rule B regex");
    let mut misses = Vec::new();
    let mut checked = 0usize;
    for (name, text) in read_documents(root) {
        let tokens: BTreeSet<String> = token_re
            .captures_iter(&text)
            .map(|caps| caps[1].to_string())
            .collect();
        for token in tokens {
            if rule_a_re.is_match(&token) {
                checked += 1;
                let whole = whole_word_regex(&token);
                if !whole.is_match(&code) {
                    misses.push(format!("{name}: `{token}` (rule A)"));
                }
            } else if let Some(caps) = rule_b_re.captures(&token) {
                let module = &caps[1];
                let item = &caps[2];
                let Some(file) = corpus.get(&format!("src/{module}.rs")) else {
                    continue;
                };
                checked += 1;
                let whole = whole_word_regex(item);
                if !whole.is_match(file) {
                    misses.push(format!("{name}: `{token}` (rule B)"));
                }
            }
        }
    }
    (misses, checked)
}

#[test]
fn docs_cite_only_code_that_exists() {
    let (misses, _) = check_documents(Path::new(env!("CARGO_MANIFEST_DIR")));
    assert!(
        misses.is_empty(),
        "docs cite code that does not exist:\n{}",
        misses.join("\n")
    );
}

#[test]
fn docs_check_covers_enough_references() {
    let (_, checked) = check_documents(Path::new(env!("CARGO_MANIFEST_DIR")));
    eprintln!("docs check covered {checked} (document, token) pairs");
    assert!(
        checked >= 200,
        "only {checked} (document, token) pairs checked, expected at least 200"
    );
}
