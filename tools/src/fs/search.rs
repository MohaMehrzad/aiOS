//! fs.search — Search for files matching a glob pattern

use anyhow::{Context, Result};
use serde_json::json;
use walkdir::WalkDir;

/// Walk the directory tree under `directory` and return every path whose
/// file-name component matches the glob `pattern`.
///
/// `max_depth` limits how many levels deep the traversal goes (0 = unlimited).
///
/// Input  JSON: `{ "directory": "/abs/dir", "pattern": "*.log", "max_depth": 5 }`
/// Output JSON: `{ "matches": ["/abs/dir/app.log", ...] }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.search: invalid JSON input")?;

    let directory = v
        .get("directory")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.search: missing required field 'directory'"))?;

    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.search: missing required field 'pattern'"))?;

    let max_depth = v
        .get("max_depth")
        .and_then(|m| m.as_u64())
        .unwrap_or(0) as usize;

    // Compile the glob pattern
    let glob = glob_pattern::Pattern::new(pattern)
        .map_err(|e| anyhow::anyhow!("fs.search: invalid glob pattern '{pattern}': {e}"))?;

    let mut walker = WalkDir::new(directory);
    if max_depth > 0 {
        walker = walker.max_depth(max_depth);
    }

    let mut matches: Vec<String> = Vec::new();

    for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue, // skip permission errors etc.
        };

        let file_name = entry.file_name().to_string_lossy();
        if glob.matches(&file_name) {
            matches.push(entry.path().to_string_lossy().to_string());
        }
    }

    let output = json!({ "matches": matches });
    serde_json::to_vec(&output).context("fs.search: failed to serialise output")
}

/// Minimal glob-pattern matcher so we don't need an extra crate.
///
/// Supports `*` (any chars), `?` (single char), and `[abc]` character classes.
mod glob_pattern {
    pub struct Pattern {
        tokens: Vec<Token>,
    }

    enum Token {
        Literal(char),
        Any,           // ?
        AnySequence,   // *
        CharClass(Vec<char>, bool), // [abc] or [!abc]
    }

    impl Pattern {
        pub fn new(pattern: &str) -> Result<Self, String> {
            let mut tokens = Vec::new();
            let mut chars = pattern.chars().peekable();

            while let Some(c) = chars.next() {
                match c {
                    '*' => tokens.push(Token::AnySequence),
                    '?' => tokens.push(Token::Any),
                    '[' => {
                        let mut class_chars = Vec::new();
                        let negated = chars.peek() == Some(&'!');
                        if negated {
                            chars.next();
                        }
                        loop {
                            match chars.next() {
                                Some(']') => break,
                                Some(ch) => class_chars.push(ch),
                                None => return Err("unterminated character class".into()),
                            }
                        }
                        tokens.push(Token::CharClass(class_chars, negated));
                    }
                    other => tokens.push(Token::Literal(other)),
                }
            }

            Ok(Self { tokens })
        }

        pub fn matches(&self, text: &str) -> bool {
            Self::do_match(&self.tokens, &text.chars().collect::<Vec<_>>(), 0, 0)
        }

        fn do_match(tokens: &[Token], text: &[char], ti: usize, si: usize) -> bool {
            let mut ti = ti;
            let mut si = si;

            while ti < tokens.len() {
                match &tokens[ti] {
                    Token::Literal(c) => {
                        if si >= text.len() || text[si] != *c {
                            return false;
                        }
                        si += 1;
                    }
                    Token::Any => {
                        if si >= text.len() {
                            return false;
                        }
                        si += 1;
                    }
                    Token::CharClass(chars, negated) => {
                        if si >= text.len() {
                            return false;
                        }
                        let found = chars.contains(&text[si]);
                        if found == *negated {
                            return false;
                        }
                        si += 1;
                    }
                    Token::AnySequence => {
                        // Try matching the rest of the pattern starting at every
                        // possible position in the remaining text.
                        for k in si..=text.len() {
                            if Self::do_match(tokens, text, ti + 1, k) {
                                return true;
                            }
                        }
                        return false;
                    }
                }
                ti += 1;
            }

            si == text.len()
        }
    }
}
