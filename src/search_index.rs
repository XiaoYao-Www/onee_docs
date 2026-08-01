//! Tantivy 全文搜尋索引 — 記憶體駐留（RAM），由檔案變更訊號驅動全量重建。
//!
//! 設計要點：
//!   - schema 使用 jieba 中文分詞（`tantivy-jieba`），中文與英文皆可檢索
//!   - 索引建於記憶體（`Index::create_in_ram`），重建 = delete_all + add + commit
//!   - `search` 只查記憶體索引，不觸碰檔案系統（避免高頻掃描）
//!   - 命中字段（title / content）以兩次獨立欄位查詢判定，snippet 由
//!     `SnippetGenerator` 產生並自動 HTML 轉義（<mark> 高亮）

use crate::articles::Article;
use serde::Serialize;
use std::io::Read;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, Term, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::TokenStream;
use tantivy::{doc, Index, IndexReader, TantivyDocument};
use tantivy_jieba::JiebaTokenizer;

/// 單筆搜尋結果（與舊實作結構相容，供 /api/search 序列化）
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub path: String,
    pub date: Option<String>,
    pub snippet: String,
    pub match_in: String,
}

/// 單欄位查詢命中的文件資料：(doc_address, path, title, date)
type FieldHit = (tantivy::DocAddress, String, String, Option<String>);

/// Tantivy 全文搜尋索引
pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    path_field: Field,
    title_field: Field,
    content_field: Field,
    date_field: Field,
}

impl SearchIndex {
    /// 建立空索引（記憶體），註冊 jieba 分詞器。
    pub fn new() -> anyhow::Result<Self> {
        let mut schema_builder = Schema::builder();
        // path：不分詞（STRING），作為刪除/去重 term 與回傳欄位
        schema_builder.add_text_field("path", STRING | STORED);
        // title：jieba 分詞，indexed + stored（回傳標題）
        schema_builder.add_text_field(
            "title",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                        .set_tokenizer("jieba"),
                )
                .set_stored(),
        );
        // content：jieba 分詞，indexed + stored（SnippetGenerator 需讀取原文）
        schema_builder.add_text_field(
            "content",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions)
                        .set_tokenizer("jieba"),
                )
                .set_stored(),
        );
        // date：不分詞，僅回傳
        schema_builder.add_text_field("date", STRING | STORED);

        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema.clone());
        // jieba 分詞（Search 模式多切詞，提升查詢命中率）+ 小寫化
        // （tantivy-jieba 本身保留英文大小寫，小寫化使「Rust」與「rust」可互相匹配，
        //   與舊實作的 case-insensitive 行為一致）
        let analyzer =
            tantivy::tokenizer::TextAnalyzer::builder(JiebaTokenizer::with_search_mode(true))
                .filter(tantivy::tokenizer::LowerCaser)
                .build();
        index.tokenizers().register("jieba", analyzer);
        let reader = index.reader()?;

        Ok(Self {
            index,
            reader,
            path_field: schema.get_field("path")?,
            title_field: schema.get_field("title")?,
            content_field: schema.get_field("content")?,
            date_field: schema.get_field("date")?,
        })
    }

    /// 全量重建索引：讀取每篇文章內容（限流、失敗跳過），
    /// `delete_all_documents` → 批次 add → `commit` → `reader.reload`。
    /// 需在 `spawn_blocking` 中呼叫（同步 I/O）。
    pub fn rebuild(&self, articles: &[Article], article_dir: &Path, max_file_size: u64) {
        let mut docs: Vec<(&Article, String)> = Vec::with_capacity(articles.len());
        for article in articles {
            match read_limited(&article_dir.join(&article.path), max_file_size) {
                Some(content) => docs.push((article, content)),
                None => continue, // 讀取失敗/超限 → 不索引該篇
            }
        }

        let mut writer = match self.index.writer(50_000_000) {
            Ok(w) => w,
            Err(err) => {
                eprintln!("⚠️  建立索引寫入器失敗: {err}");
                return;
            }
        };
        if let Err(err) = writer.delete_all_documents() {
            eprintln!("⚠️  清除索引失敗: {err}");
            return;
        }
        for (article, content) in &docs {
            let date = article.date.as_deref().unwrap_or("");
            let doc = doc!(
                self.path_field => article.path.as_str(),
                self.title_field => article.title.as_str(),
                self.content_field => content.as_str(),
                self.date_field => date
            );
            if let Err(err) = writer.add_document(doc) {
                eprintln!("⚠️  索引文章失敗 ({}): {err}", article.path);
            }
        }
        if let Err(err) = writer.commit() {
            eprintln!("⚠️  提交索引失敗: {err}");
            return;
        }
        if let Err(err) = self.reader.reload() {
            eprintln!("⚠️  重載索引失敗: {err}");
        }
    }

    /// 搜尋：分別查 title / content 欄位並合併，回傳最多 `max_results` 條。
    /// 純記憶體操作，不觸碰檔案系統。查詢解析失敗時回退為短語查詢，再失敗回空。
    pub fn search(&self, q: &str, max_results: usize) -> Vec<SearchResult> {
        let searcher = self.reader.searcher();

        // 1) title 欄位命中（優先展示）
        let mut results: Vec<SearchResult> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Some(docs) = self.search_field(&searcher, q, self.title_field, max_results) {
            for (_doc_address, path, title, date) in docs {
                if results.len() >= max_results {
                    break;
                }
                if !seen.insert(path.clone()) {
                    continue;
                }
                results.push(SearchResult {
                    title: title.clone(),
                    path: path.clone(),
                    date,
                    snippet: String::new(), // 檔名命中 → 無 snippet（與舊行為一致）
                    match_in: "title".to_string(),
                });
            }
        }

        // 2) content 欄位命中（附 snippet 高亮），排除已在 title 結果中的
        if results.len() < max_results {
            if let Some(docs) = self.search_field(&searcher, q, self.content_field, max_results) {
                for (doc_address, path, title, date) in docs {
                    if results.len() >= max_results {
                        break;
                    }
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    let snippet = self.build_snippet(&searcher, q, doc_address);
                    results.push(SearchResult {
                        title,
                        path,
                        date,
                        snippet,
                        match_in: "content".to_string(),
                    });
                }
            }
        }

        results
    }

    /// 對單一欄位執行查詢，回傳 (doc_address, path, title, date)。
    /// 查詢解析失敗 → 嘗試短語查詢；仍失敗 → None。
    fn search_field(
        &self,
        searcher: &tantivy::Searcher,
        q: &str,
        field: Field,
        limit: usize,
    ) -> Option<Vec<FieldHit>> {
        let query = self.parse_query(q, field)?;
        let top_docs: Vec<(f32, tantivy::DocAddress)> = searcher
            .search(&query, &TopDocs::with_limit(limit).order_by_score())
            .ok()?;

        let mut out = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address).ok()?;
            let path = doc
                .get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = doc
                .get_first(self.title_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let date = doc
                .get_first(self.date_field)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            out.push((doc_address, path, title, date));
        }
        Some(out)
    }

    /// 解析查詢：用索引的 jieba 分詞器切分查詢詞，對每個 token 建 TermQuery，
    /// 多詞以 OR（Should）組合。相較於 `QueryParser`（無空格連續 token 會
    /// 被視為相鄰短語，中文子詞切分後往往不相鄰而漏配），OR 語義與舊實作的
    /// 子字串匹配更接近，也對使用者輸入更寬容。
    fn parse_query(&self, q: &str, field: Field) -> Option<Box<dyn Query>> {
        let mut analyzer = self.index.tokenizers().get("jieba")?;
        let mut tokens: Vec<String> = Vec::new();
        let mut stream = analyzer.token_stream(q);
        while let Some(tok) = stream.next() {
            let text = tok.text.to_string();
            if !text.trim().is_empty() && !tokens.contains(&text) {
                tokens.push(text);
            }
        }
        if tokens.is_empty() {
            return None;
        }
        if tokens.len() == 1 {
            return Some(Box::new(TermQuery::new(
                Term::from_field_text(field, &tokens[0]),
                IndexRecordOption::WithFreqsAndPositions,
            )));
        }
        let subqueries: Vec<(Occur, Box<dyn Query>)> = tokens
            .into_iter()
            .map(|t| {
                (
                    Occur::Should,
                    Box::new(TermQuery::new(
                        Term::from_field_text(field, &t),
                        IndexRecordOption::WithFreqsAndPositions,
                    )) as Box<dyn Query>,
                )
            })
            .collect();
        Some(Box::new(BooleanQuery::new(subqueries)))
    }

    /// 產生 content 命中片段：<mark> 高亮、最多 200 字元（SnippetGenerator 自動 HTML 轉義）。
    fn build_snippet(
        &self,
        searcher: &tantivy::Searcher,
        q: &str,
        doc_address: tantivy::DocAddress,
    ) -> String {
        let query = match self.parse_query(q, self.content_field) {
            Some(q) => q,
            None => return String::new(),
        };
        let mut generator = match SnippetGenerator::create(searcher, &query, self.content_field) {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        generator.set_max_num_chars(200);
        let doc: TantivyDocument = match searcher.doc(doc_address) {
            Ok(d) => d,
            Err(_) => return String::new(),
        };
        let mut snippet = generator.snippet_from_doc(&doc);
        snippet.set_snippet_prefix_postfix("<mark>", "</mark>");
        snippet.to_html()
    }
}

/// 限流讀取檔案內容（TOCTOU 防護）：最多讀 `max_file_size + 1` 位元組，
/// 超限或讀取失敗回傳 None（該篇不索引）。
fn read_limited(path: &Path, max_file_size: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(max_file_size + 1).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 > max_file_size {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_article_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_idx_test_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_articles() -> (std::path::PathBuf, Vec<Article>) {
        let dir = temp_article_dir("search");
        let sub = dir.join("knowledges").join("20260730");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("learning-notes.md"),
            "今天學習了 Rust 的所有權與借用。",
        )
        .unwrap();
        fs::write(dir.join("關於本站.md"), "這是本站簡介，沒有任何關鍵字。").unwrap();
        fs::write(dir.join("notes.txt"), "Rust 不應該被搜到").unwrap();

        let articles = vec![
            Article {
                title: "learning notes".to_string(),
                path: "knowledges/20260730/learning-notes.md".to_string(),
                date: Some("2026-07-30".to_string()),
            },
            Article {
                title: "關於本站".to_string(),
                path: "關於本站.md".to_string(),
                date: None,
            },
        ];
        (dir, articles)
    }

    #[test]
    fn test_search_english_content() {
        let (dir, articles) = sample_articles();
        let idx = SearchIndex::new().unwrap();
        idx.rebuild(&articles, &dir, 5 * 1024 * 1024);

        let results = idx.search("rust", 20);
        assert_eq!(results.len(), 1, "英文內容應命中 1 篇");
        let r = &results[0];
        assert_eq!(r.path, "knowledges/20260730/learning-notes.md");
        assert_eq!(r.match_in, "content");
        assert!(
            r.snippet.contains("<mark>Rust</mark>"),
            "snippet: {}",
            r.snippet
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_search_chinese_content() {
        let (dir, articles) = sample_articles();
        let idx = SearchIndex::new().unwrap();
        idx.rebuild(&articles, &dir, 5 * 1024 * 1024);

        // jieba 分詞：搜「所有權」應命中「所有權與借用」（OR 子詞匹配）
        let results = idx.search("所有權", 20);
        assert_eq!(results.len(), 1, "中文內容應命中 1 篇");
        let r = &results[0];
        assert_eq!(r.match_in, "content");
        assert!(r.snippet.contains("<mark>"), "snippet: {}", r.snippet);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_search_title_only() {
        let (dir, articles) = sample_articles();
        let idx = SearchIndex::new().unwrap();
        idx.rebuild(&articles, &dir, 5 * 1024 * 1024);

        let results = idx.search("learning", 20);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_in, "title");
        assert!(results[0].snippet.is_empty(), "檔名命中不應有 snippet");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_search_no_match() {
        let (dir, articles) = sample_articles();
        let idx = SearchIndex::new().unwrap();
        idx.rebuild(&articles, &dir, 5 * 1024 * 1024);

        assert!(idx.search("xyzzy-no-such-term", 20).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_search_max_results_limit() {
        let dir = temp_article_dir("limit");
        let mut articles = Vec::new();
        for i in 0..5 {
            let fname = format!("note-{i}.md");
            fs::write(dir.join(&fname), format!("共同關鍵字 common {i}")).unwrap();
            articles.push(Article {
                title: format!("note {i}"),
                path: fname,
                date: None,
            });
        }
        let idx = SearchIndex::new().unwrap();
        idx.rebuild(&articles, &dir, 5 * 1024 * 1024);

        let results = idx.search("common", 3);
        assert_eq!(results.len(), 3, "結果應被上限截斷");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_rebuild_updates_index() {
        let dir = temp_article_dir("rebuild");
        fs::write(dir.join("a.md"), "alpha 內容").unwrap();
        let idx = SearchIndex::new().unwrap();

        let a1 = vec![Article {
            title: "a".to_string(),
            path: "a.md".to_string(),
            date: None,
        }];
        idx.rebuild(&a1, &dir, 5 * 1024 * 1024);
        assert_eq!(idx.search("alpha", 20).len(), 1);

        // 刪除舊文、新增新文 → 重建後索引同步
        fs::remove_file(dir.join("a.md")).unwrap();
        fs::write(dir.join("b.md"), "beta 內容").unwrap();
        let a2 = vec![Article {
            title: "b".to_string(),
            path: "b.md".to_string(),
            date: None,
        }];
        idx.rebuild(&a2, &dir, 5 * 1024 * 1024);

        assert!(idx.search("alpha", 20).is_empty(), "舊文應已移除");
        assert_eq!(idx.search("beta", 20).len(), 1, "新文應可搜到");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_over_max_size_not_indexed() {
        let dir = temp_article_dir("oversize");
        fs::write(dir.join("big.md"), "0123456789").unwrap();
        let idx = SearchIndex::new().unwrap();
        let articles = vec![Article {
            title: "big".to_string(),
            path: "big.md".to_string(),
            date: None,
        }];
        // 上限 4 位元組 → 10 位元組內容不索引
        idx.rebuild(&articles, &dir, 4);
        assert!(idx.search("0123456789", 20).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
