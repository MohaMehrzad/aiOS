//! web.scrape — Fetch a web page and extract text content

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    url: String,
    /// CSS selector to extract specific elements (optional)
    #[serde(default)]
    selector: String,
    /// Maximum content length to return
    #[serde(default = "default_max_len")]
    max_length: usize,
}

fn default_max_len() -> usize {
    50000
}

#[derive(Serialize)]
struct Output {
    url: String,
    title: String,
    text: String,
    content_length: usize,
    truncated: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Fetch the page with curl
    let output = Command::new("curl")
        .args(["-s", "-S", "-L", "--max-time", "15", &input.url])
        .output()
        .with_context(|| format!("Failed to fetch URL: {}", input.url))?;

    let html = String::from_utf8_lossy(&output.stdout).to_string();

    // Extract title from <title> tag
    let title = extract_between(&html, "<title>", "</title>")
        .or_else(|| extract_between(&html, "<title ", ">").and_then(|_| extract_between(&html, ">", "</title>")))
        .unwrap_or_default()
        .trim()
        .to_string();

    // Extract text content
    let text = if input.selector.is_empty() {
        // Strip all HTML tags and extract text
        strip_html_tags(&html)
    } else {
        // Simple CSS selector extraction (supports tag, .class, #id)
        extract_by_selector(&html, &input.selector)
    };

    // Clean up whitespace
    let text = collapse_whitespace(&text);
    let content_length = text.len();
    let truncated = content_length > input.max_length;
    let text = if truncated {
        text[..input.max_length].to_string()
    } else {
        text
    };

    let result = Output {
        url: input.url,
        title,
        text,
        content_length,
        truncated,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

/// Extract text between two markers
fn extract_between<'a>(html: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = html.find(start)? + start.len();
    let end_idx = html[start_idx..].find(end)? + start_idx;
    Some(&html[start_idx..end_idx])
}

/// Strip HTML tags and return plain text
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if !in_tag && i + 7 < lower_chars.len() {
            let ahead: String = lower_chars[i..i + 7].iter().collect();
            if ahead == "<script" {
                in_script = true;
            } else if ahead == "<style " || (i + 6 < lower_chars.len() && lower_chars[i..i + 6].iter().collect::<String>() == "<style") {
                in_style = true;
            }
        }

        if chars[i] == '<' {
            // Check for end of script/style
            if i + 9 < lower_chars.len() {
                let ahead: String = lower_chars[i..i + 9].iter().collect();
                if ahead == "</script>" {
                    in_script = false;
                }
            }
            if i + 8 < lower_chars.len() {
                let ahead: String = lower_chars[i..i + 8].iter().collect();
                if ahead == "</style>" {
                    in_style = false;
                }
            }
            in_tag = true;
        } else if chars[i] == '>' {
            in_tag = false;
        } else if !in_tag && !in_script && !in_style {
            result.push(chars[i]);
        }

        i += 1;
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Extract content matching a simple CSS selector
fn extract_by_selector(html: &str, selector: &str) -> String {
    let tag = if selector.starts_with('.') || selector.starts_with('#') {
        // Class or ID selector — search for elements with that attribute
        let attr_type = if selector.starts_with('.') { "class" } else { "id" };
        let value = &selector[1..];
        // Find elements with matching attribute
        let pattern = format!("{attr_type}=\"{value}\"");
        let mut results = Vec::new();

        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find(&pattern) {
            let abs_pos = search_from + pos;
            // Find the opening < before this attribute
            if let Some(_tag_start) = html[..abs_pos].rfind('<') {
                // Find the matching >
                if let Some(tag_end) = html[abs_pos..].find('>') {
                    let content_start = abs_pos + tag_end + 1;
                    // Find the closing tag
                    if let Some(close) = html[content_start..].find("</") {
                        let content = &html[content_start..content_start + close];
                        results.push(strip_html_tags(content));
                    }
                }
            }
            search_from = abs_pos + pattern.len();
        }
        return results.join("\n");
    } else {
        selector
    };

    // Tag selector
    let open_tag = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut results = Vec::new();
    let mut search_from = 0;

    while let Some(pos) = html[search_from..].find(&open_tag) {
        let abs_pos = search_from + pos;
        if let Some(tag_end) = html[abs_pos..].find('>') {
            let content_start = abs_pos + tag_end + 1;
            if let Some(close) = html[content_start..].find(&close_tag) {
                let content = &html[content_start..content_start + close];
                results.push(strip_html_tags(content));
            }
        }
        search_from = abs_pos + open_tag.len();
        if results.len() >= 50 {
            break;
        }
    }

    results.join("\n")
}

/// Collapse multiple whitespace chars into single spaces
fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }

    result.trim().to_string()
}
