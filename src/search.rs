//! 搜尋邏輯（檔名 + 內容）— 行為與原 Python 版 `server.py` 一致

use crate::articles::{extract_date, extract_title, walk_files};
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 內容最多讀取前 100K 字元（與 Python 文本模式 `f.read(1024 * 100)` 一致）
const CONTENT_READ_LIMIT_CHARS: usize = 1024 * 100;
/// UTF-8 單一字元最多 4 位元組，讀取上限位元組數
const CONTENT_READ_BYTES: usize = CONTENT_READ_LIMIT_CHARS * 4;
/// 匹配位置前後各 60 字元
const SNIPPET_PADDING: usize = 60;
/// 片段長度上限
const SNIPPET_MAX_LEN: usize = 200;

/// 單筆搜尋結果
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub path: String,
    pub date: Option<String>,
    pub snippet: String,
    pub match_in: String,
}

/// 搜尋文章（檔名 + 內容），最多回傳 `max_results` 條結果。
/// `q` 已由呼叫端驗證（長度 1-200、無空字節）。
pub fn search_articles(article_dir: &Path, q: &str, max_results: usize) -> Vec<SearchResult> {
    // regex::escape 防止 ReDoS；`(?i)` 對應 Python re.IGNORECASE
    let escaped = regex::escape(q);
    let re = Regex::new(&format!("(?i){escaped}")).expect("escaped pattern is always valid");

    if !article_dir.is_dir() {
        return Vec::new();
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if let Err(err) = walk_files(article_dir, &mut files) {
        eprintln!("⚠️  掃描文章目錄失敗: {err}");
        return Vec::new();
    }

    let mut results: Vec<SearchResult> = Vec::new();
    for full_path in files {
        if results.len() >= max_results {
            break;
        }
        let fname = match full_path.file_name().and_then(|s| s.to_str()) {
            Some(name) if name.ends_with(".md") => name,
            _ => continue,
        };

        let rel_path = full_path.strip_prefix(article_dir).unwrap_or(&full_path);
        let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");

        let title = extract_title(fname);
        let parent_name = full_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let date = extract_date(parent_name);

        // 比對檔名
        let mut match_in: Vec<&str> = Vec::new();
        if re.is_match(fname) {
            match_in.push("title");
        }

        // 比對內容（只讀前 100K 字元）；讀取失敗 → 跳過該檔案（與 Python 一致）
        let content = match read_limited(&full_path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let mut content_match = false;
        let mut snippet = String::new();
        if re.is_match(&content) {
            content_match = true;
            match_in.push("content");
            snippet = build_snippet(&content, &re);
        }

        if match_in.is_empty() {
            continue;
        }

        results.push(SearchResult {
            title,
            path: rel_path_str,
            date,
            snippet: if content_match { snippet } else { String::new() },
            match_in: match_in.join("|"),
        });
    }

    results.truncate(max_results);
    results
}

/// 讀取檔案最多前 `CONTENT_READ_LIMIT_CHARS` 個字元。
/// 先讀足量位元組（UTF-8 最壞 4 位元組/字元），再按字元截斷，
/// 與 Python 文本模式 `read(n)` 的字元語義一致。
fn read_limited(path: &Path) -> std::io::Result<String> {
    let f = fs::File::open(path)?;
    let mut buf = Vec::with_capacity(CONTENT_READ_BYTES);
    f.take(CONTENT_READ_BYTES as u64).read_to_end(&mut buf)?;
    // 位元組截斷邊界若切在多字元中間，尾端以 U+FFFD 取代（對應 errors="replace"）；
    // 前 100K 個字元永遠是完整的，不影響結果
    let s = String::from_utf8_lossy(&buf);
    Ok(s.chars().take(CONTENT_READ_LIMIT_CHARS).collect())
}

/// 構建 snippet：取第一個匹配位置前後各 60 字元，換行轉空格，
/// 用 `<mark>` 標記所有匹配位置，超過 200 字元截斷並加 `…`。
pub(crate) fn build_snippet(content: &str, re: &Regex) -> String {
    let m = match re.find(content) {
        Some(m) => m,
        None => return String::new(),
    };

    // 位元組偏移 → 字元偏移（與 Python 的字元切片語義一致）
    let start_char = content[..m.start()].chars().count();
    let end_char = content[..m.end()].chars().count();

    let chars: Vec<char> = content.chars().collect();
    let start = start_char.saturating_sub(SNIPPET_PADDING);
    let end = (end_char + SNIPPET_PADDING).min(chars.len());

    let mut snippet: String = chars[start..end].iter().collect();
    snippet = snippet.replace('\n', " ");
    // 用 <mark> 標記關鍵字（$0 保留原文，大小寫不敏感時亦一致）
    snippet = re.replace_all(&snippet, "<mark>$0</mark>").into_owned();

    if snippet.chars().count() > SNIPPET_MAX_LEN {
        snippet = snippet.chars().take(SNIPPET_MAX_LEN).collect::<String>() + "…";
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_test_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_search_title_and_content() {
        let dir = temp_dir("search");

        let sub = dir.join("knowledges").join("20260730");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("learning-notes.md"), "今天學習了 Rust 的所有權與借用。").unwrap();
        fs::write(dir.join("關於本站.md"), "這是本站簡介，沒有任何關鍵字。").unwrap();
        // 非 .md 檔案不參與搜尋
        fs::write(dir.join("notes.txt"), "Rust 不應該被搜到").unwrap();

        // 內容匹配（大小寫不敏感）
        let results = search_articles(&dir, "rust", 20);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.title, "learning notes");
        assert_eq!(r.path, "knowledges/20260730/learning-notes.md");
        assert_eq!(r.date.as_deref(), Some("2026-07-30"));
        assert!(r.snippet.contains("<mark>Rust</mark>"));
        assert_eq!(r.match_in, "content");

        // 檔名匹配
        let results = search_articles(&dir, "learning", 20);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_in, "title");
        assert!(results[0].snippet.is_empty());

        // 無匹配
        let results = search_articles(&dir, "xyzzy", 20);
        assert!(results.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_search_empty_dir() {
        let dir = temp_dir("search_empty");
        assert!(search_articles(&dir, "rust", 20).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_build_snippet_padding() {
        let re = Regex::new("(?i)rust").unwrap();
        let content = format!("{}Rust{}", "a".repeat(80), "b".repeat(80));
        let snippet = build_snippet(&content, &re);
        assert!(snippet.contains("<mark>Rust</mark>"));
        assert!(snippet.starts_with('a'));
        assert!(snippet.ends_with('b'));
        // 前 60 字元為 a，後 60 字元為 b
        assert_eq!(snippet.chars().take(60).collect::<String>(), "a".repeat(60));
        assert_eq!(
            snippet.chars().rev().take(60).collect::<String>(),
            "b".repeat(60).chars().rev().collect::<String>()
        );
    }

    #[test]
    fn test_build_snippet_newline_replaced() {
        let re = Regex::new("(?i)rust").unwrap();
        let content = "第一行\n第二行 Rust 結尾";
        let snippet = build_snippet(content, &re);
        assert!(!snippet.contains('\n'));
        assert!(snippet.contains("<mark>Rust</mark>"));
    }

    #[test]
    fn test_build_snippet_truncation() {
        // 匹配本身很長 → snippet 超過 200 字元 → 截斷加 …
        let re = Regex::new("x+").unwrap();
        let content = "x".repeat(250);
        let snippet = build_snippet(&content, &re);
        assert_eq!(snippet.chars().count(), 201);
        assert!(snippet.ends_with('…'));
    }
}
