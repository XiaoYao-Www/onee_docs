//! 文章掃描邏輯 — 行為與原 Python 版 `server.py` / `scripts/generate_index.py` 一致

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// 單篇文章資訊
#[derive(Debug, Clone, Serialize)]
pub struct Article {
    pub title: String,
    pub path: String,
    pub date: Option<String>,
}

/// 從檔名推斷標題：去掉 `.md`，將 `-`/`_` 取代為空格。
pub fn extract_title(filename: &str) -> String {
    let name = filename.strip_suffix(".md").unwrap_or(filename);
    let name = name.replace(['-', '_'], " ");
    let name = name.trim();
    if name.is_empty() {
        filename.to_string()
    } else {
        name.to_string()
    }
}

/// 從上層目錄名稱提取日期：`YYYYMMDD` → `YYYY-MM-DD`，非 8 位數字則為 `None`。
pub(crate) fn extract_date(parent_name: &str) -> Option<String> {
    if parent_name.len() == 8 && parent_name.chars().all(|c| c.is_ascii_digit()) {
        Some(format!(
            "{}-{}-{}",
            &parent_name[..4],
            &parent_name[4..6],
            &parent_name[6..8]
        ))
    } else {
        None
    }
}

/// 遞迴收集 `dir` 下所有檔案路徑（不追蹤符號連結目錄與檔案，與 Python
/// `os.walk` 預設一致，並確保 scan/search 不會讀取 article/ 外的內容）。
pub(crate) fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            walk_files(&path, out)?;
        } else if file_type.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// 掃描 `article_dir` 下所有 `.md` 檔案並排序：
/// 無日期置頂（依標題升冪），有日期依日期降冪。
pub(crate) fn scan_articles(article_dir: &Path) -> Vec<Article> {
    if !article_dir.is_dir() {
        return Vec::new();
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if let Err(err) = walk_files(article_dir, &mut files) {
        eprintln!("⚠️  掃描文章目錄失敗: {err}");
        return Vec::new();
    }

    // 與 Python `sorted(files)` 一致：依檔名排序
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut articles: Vec<Article> = Vec::new();
    for full_path in files {
        let fname = match full_path.file_name().and_then(|s| s.to_str()) {
            Some(name) if name.ends_with(".md") => name,
            _ => continue,
        };

        let rel_path = full_path.strip_prefix(article_dir).unwrap_or(&full_path);
        let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");

        let parent_name = full_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("");

        articles.push(Article {
            title: extract_title(fname),
            path: rel_path_str,
            date: extract_date(parent_name),
        });
    }

    // 排序：無日期置頂（依標題升冪），有日期依降冪
    let mut no_date: Vec<Article> = Vec::new();
    let mut with_date: Vec<Article> = Vec::new();
    for a in articles {
        if a.date.is_some() {
            with_date.push(a);
        } else {
            no_date.push(a);
        }
    }
    no_date.sort_by(|a, b| a.title.cmp(&b.title));
    with_date.sort_by(|a, b| b.date.cmp(&a.date));
    no_date.extend(with_date);
    no_date
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_article_dir(tag: &str) -> PathBuf {
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
    fn test_extract_title() {
        assert_eq!(extract_title("learning-notes.md"), "learning notes");
        assert_eq!(extract_title("關於本站.md"), "關於本站");
        assert_eq!(extract_title("a_b_c.md"), "a b c");
        // 空標題回退到原檔名
        assert_eq!(extract_title(".md"), ".md");
    }

    #[test]
    fn test_extract_date() {
        assert_eq!(extract_date("20260730").as_deref(), Some("2026-07-30"));
        assert_eq!(extract_date("2026073").as_deref(), None);
        assert_eq!(extract_date("2026073a").as_deref(), None);
        assert_eq!(extract_date("knowledges").as_deref(), None);
    }

    #[test]
    fn test_scan_articles_sorting() {
        let dir = temp_article_dir("scan");

        // 無日期（置頂）
        fs::write(dir.join("zeta.md"), "# zeta").unwrap();
        fs::write(dir.join("alpha.md"), "# alpha").unwrap();
        // 有日期
        let d1 = dir.join("knowledges").join("20250101");
        let d2 = dir.join("knowledges").join("20260730");
        fs::create_dir_all(&d1).unwrap();
        fs::create_dir_all(&d2).unwrap();
        fs::write(d1.join("old.md"), "# old").unwrap();
        fs::write(d2.join("new.md"), "# new").unwrap();
        // 非 .md 忽略
        fs::write(dir.join("notes.txt"), "ignore me").unwrap();

        let articles = scan_articles(&dir);
        let titles: Vec<&str> = articles.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["alpha", "zeta", "new", "old"]);
        assert_eq!(articles[2].date.as_deref(), Some("2026-07-30"));
        assert_eq!(articles[3].date.as_deref(), Some("2025-01-01"));
        assert_eq!(articles[0].path, "alpha.md");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_scan_articles_missing_dir() {
        let dir = std::env::temp_dir().join(format!("daily_knowledge_no_such_{}", std::process::id()));
        assert!(scan_articles(&dir).is_empty());
    }
}
