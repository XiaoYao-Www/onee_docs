//! 伺服器設定 — 從 `appdata/config/server_config.toml` 讀取
//!
//! 設定檔為選配：檔案不存在時使用內建預設值；欄位缺失時各欄位回退其預設值。
//! 解析失敗（語法錯誤）時視為設定錯誤，伺服器拒絕啟動。

use serde::Deserialize;
use std::path::Path;

// ── 內建預設值 ──────────────────────────────────────────

/// 預設埠號（無 CLI / 環境變數 / 設定檔指定時）
pub const DEFAULT_PORT: u16 = 8765;
/// 預設：不允許 Markdown 內嵌 HTML（安全預設，防止文章內容執行任意腳本）
pub const DEFAULT_ALLOW_MARKDOWN_HTML: bool = false;
/// 預設單檔大小上限：5MB
pub const DEFAULT_MAX_FILE_SIZE: usize = 5 * 1024 * 1024;
/// 預設限速窗口：60 秒
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
/// 預設限速閾值：每窗口 300 次請求
pub const DEFAULT_RATE_LIMIT_MAX_REQUESTS: u32 = 300;
/// 預設搜尋結果上限
pub const DEFAULT_SEARCH_MAX_RESULTS: usize = 20;
/// 預設定時掃描間隔（秒）：兜底 notify 漏報
pub const DEFAULT_PERIODIC_SCAN_INTERVAL_SECS: u64 = 3600;
/// 預設導覽列（側邊欄）標題
pub const DEFAULT_SITE_TITLE: &str = "每日知識庫";
/// 預設瀏覽器分頁標題
pub const DEFAULT_PAGE_TITLE: &str = "每日知識庫";

// ── 設定結構 ────────────────────────────────────────────

/// 頂層伺服器設定
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// 監聽埠號；`None` 表示未在設定檔指定，交由 CLI / 環境變數 / 預設值決定
    pub port: Option<u16>,
    /// 是否允許 Markdown 內嵌 HTML 原樣渲染（`false` 為安全預設）
    pub allow_markdown_html: bool,
    /// 單篇文章讀取上限（位元組）
    pub max_file_size: usize,
    /// API 限速設定
    pub rate_limit: RateLimitConfig,
    /// 搜尋行為設定
    pub search: SearchConfig,
    /// 定時掃描文章目錄間隔（秒）
    pub periodic_scan_interval_secs: u64,
    /// 導覽列（側邊欄）標題
    pub site_title: String,
    /// 瀏覽器分頁標題
    pub page_title: String,
}

/// API 限速設定（每 IP 滑動窗口）
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    /// 窗口長度（秒）
    pub window_secs: u64,
    /// 窗口內允許的最大請求數
    pub max_requests: u32,
}

/// 搜尋行為設定
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// 搜尋結果上限
    pub max_results: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: None,
            allow_markdown_html: DEFAULT_ALLOW_MARKDOWN_HTML,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            rate_limit: RateLimitConfig::default(),
            search: SearchConfig::default(),
            periodic_scan_interval_secs: DEFAULT_PERIODIC_SCAN_INTERVAL_SECS,
            site_title: DEFAULT_SITE_TITLE.to_string(),
            page_title: DEFAULT_PAGE_TITLE.to_string(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            window_secs: DEFAULT_RATE_LIMIT_WINDOW_SECS,
            max_requests: DEFAULT_RATE_LIMIT_MAX_REQUESTS,
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_SEARCH_MAX_RESULTS,
        }
    }
}

impl ServerConfig {
    /// 設定檔路徑：`BASE_DIR/appdata/config/server_config.toml`
    pub fn config_file_path(base_dir: &Path) -> std::path::PathBuf {
        base_dir
            .join("appdata")
            .join("config")
            .join("server_config.toml")
    }

    /// 從 `path` 載入設定。
    /// - 檔案不存在 → 內建預設值
    /// - 檔案存在但語法/型別錯誤 → `Err`（呼叫端應中止啟動）
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let config: ServerConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("設定檔 {} 解析失敗: {e}", path.display()))?;
        Ok(config)
    }
}

// ── 埠號解析（CLI > 環境變數 > 設定檔 > 預設值） ────────

/// 埠號優先級：`cli` > 環境變數 `PORT` > 設定檔 `port` > `DEFAULT_PORT`。
/// 無效值（非數字 / 超出 u16 範圍）會略過並退回下一層。
pub fn resolve_port(cli: Option<&str>, env_port: Option<&str>, config_port: Option<u16>) -> u16 {
    if let Some(p) = cli {
        if let Ok(port) = p.trim().parse::<u16>() {
            return port;
        }
    }
    if let Some(p) = env_port {
        if let Ok(port) = p.trim().parse::<u16>() {
            return port;
        }
    }
    config_port.unwrap_or(DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let c = ServerConfig::default();
        assert_eq!(c.port, None);
        assert!(!c.allow_markdown_html);
        assert_eq!(c.max_file_size, DEFAULT_MAX_FILE_SIZE);
        assert_eq!(c.rate_limit.window_secs, 60);
        assert_eq!(c.rate_limit.max_requests, 300);
        assert_eq!(c.search.max_results, 20);
        assert_eq!(c.site_title, DEFAULT_SITE_TITLE);
        assert_eq!(c.page_title, DEFAULT_PAGE_TITLE);
        assert_eq!(c.periodic_scan_interval_secs, DEFAULT_PERIODIC_SCAN_INTERVAL_SECS);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let p = Path::new("no-such-config-dir-xyz/server_config.toml");
        let c = ServerConfig::load(p).unwrap();
        assert_eq!(c.allow_markdown_html, DEFAULT_ALLOW_MARKDOWN_HTML);
    }

    #[test]
    fn test_load_partial_toml_uses_defaults_for_missing_fields() {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cfg_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server_config.toml");
        std::fs::write(&path, "allow_markdown_html = true\n").unwrap();

        let c = ServerConfig::load(&path).unwrap();
        assert!(c.allow_markdown_html);
        assert_eq!(c.port, None); // 未指定
        assert_eq!(c.max_file_size, DEFAULT_MAX_FILE_SIZE); // 回退預設
        assert_eq!(c.search.max_results, 20);
        assert_eq!(c.site_title, DEFAULT_SITE_TITLE); // 未指定 → 回退預設
        assert_eq!(c.page_title, DEFAULT_PAGE_TITLE);
        assert_eq!(c.periodic_scan_interval_secs, DEFAULT_PERIODIC_SCAN_INTERVAL_SECS);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_load_full_toml() {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cfg2_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server_config.toml");
        std::fs::write(
            &path,
            r#"
port = 9000
allow_markdown_html = true
max_file_size = 1048576
site_title = "我的知識庫"
page_title = "我的知識庫"
periodic_scan_interval_secs = 7200

[rate_limit]
window_secs = 30
max_requests = 100

[search]
max_results = 5
"#,
        )
        .unwrap();

        let c = ServerConfig::load(&path).unwrap();
        assert_eq!(c.port, Some(9000));
        assert!(c.allow_markdown_html);
        assert_eq!(c.max_file_size, 1048576);
        assert_eq!(c.site_title, "我的知識庫");
        assert_eq!(c.page_title, "我的知識庫");
        assert_eq!(c.periodic_scan_interval_secs, 7200);
        assert_eq!(c.rate_limit.window_secs, 30);
        assert_eq!(c.rate_limit.max_requests, 100);
        assert_eq!(c.search.max_results, 5);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_load_invalid_toml_errors() {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cfg3_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server_config.toml");
        std::fs::write(&path, "allow_markdown_html = not-a-bool\n").unwrap();

        assert!(ServerConfig::load(&path).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_port_precedence() {
        // CLI > 環境變數 > 設定檔 > 預設
        assert_eq!(resolve_port(Some("3000"), Some("8080"), Some(9000)), 3000);
        assert_eq!(resolve_port(None, Some("8080"), Some(9000)), 8080);
        assert_eq!(resolve_port(None, None, Some(9000)), 9000);
        assert_eq!(resolve_port(None, None, None), DEFAULT_PORT);
        // 無效 CLI → 退回下一層
        assert_eq!(resolve_port(Some("abc"), Some("8080"), Some(9000)), 8080);
        assert_eq!(resolve_port(Some("99999"), None, Some(9000)), 9000);
    }

    #[test]
    fn test_top_level_keys_after_table_are_swallowed() {
        // 防回歸: TOML 規範中, [search] 表後的裸 key 屬於該表。
        // 若 site_title/page_title 誤放在表後, 會被歸入 [search] 而回退默認值。
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cfg_table_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server_config.toml");
        std::fs::write(
            &path,
            r#"
port = 9000
allow_markdown_html = true

[rate_limit]
window_secs = 30
max_requests = 100

[search]
max_results = 5
site_title = "誤入表內"
page_title = "誤入表內"
"#,
        )
        .unwrap();

        let c = ServerConfig::load(&path).unwrap();
        // 被 [search] 吞掉 → 頂層欄位回退默認值
        assert_eq!(c.site_title, DEFAULT_SITE_TITLE);
        assert_eq!(c.page_title, DEFAULT_PAGE_TITLE);
        assert_eq!(c.search.max_results, 5);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_top_level_keys_before_tables_apply() {
        // 正確寫法: 頂層欄位必須放在所有 [表] 定義之前才會生效
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cfg_top_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server_config.toml");
        std::fs::write(
            &path,
            r#"
site_title = "我的知識庫"
page_title = "我的知識庫"

[rate_limit]
window_secs = 30

[search]
max_results = 5
"#,
        )
        .unwrap();

        let c = ServerConfig::load(&path).unwrap();
        assert_eq!(c.site_title, "我的知識庫");
        assert_eq!(c.page_title, "我的知識庫");
        assert_eq!(c.rate_limit.window_secs, 30);
        assert_eq!(c.search.max_results, 5);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
