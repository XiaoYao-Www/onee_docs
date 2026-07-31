//! daily-knowledge — 每日知識庫輕量後端伺服器（Rust 版）
//!
//! 功能（與原 Python 版 `server.py` 行為一致）：
//!   - 提供靜態檔案服務 (index.html, style.css, script.js 等)
//!   - GET /api/articles       → 動態掃描 article/ 回傳 JSON 文章列表
//!   - GET /api/article?path=… → 安全回傳指定 .md 檔案內容
//!   - GET /api/search?q=…     → 搜尋文章（檔名 + 內容）
//!
//! 安全防護（逐條落實）：
//!   - 路徑穿越：canonicalize + 前綴檢查（對應 Python realpath + startswith）
//!   - 僅允許 .md 檔案
//!   - 空字節拒絕
//!   - 檔案大小上限 5MB
//!   - 不拼接任何 shell 指令

mod articles;
mod cache;
mod config;
mod search_index;

use articles::Article;
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use percent_encoding::percent_decode_str;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tower_http::services::ServeDir;

#[derive(Parser, Debug)]
#[command(name = "daily-knowledge", about = "每日知識庫輕量後端伺服器")]
struct Args {
    /// 指定埠號（預設 8765；無效值時退回環境變數 / 預設值）
    #[arg(long)]
    port: Option<String>,
}

/// 專案根目錄（靜態檔案來源，對應 Python 的 BASE_DIR）
fn resolve_base_dir() -> PathBuf {
    // 1. 環境變數 BASE_DIR 優先（可用於容器掛載場景）
    if let Ok(dir) = env::var("BASE_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    // 2. 可執行檔所在目錄（Docker：binary 與前端檔案同目錄）
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("index.html").is_file() {
                return dir.to_path_buf();
            }
        }
    }
    // 3. 回退到當前工作目錄（本地開發通常從專案根目錄執行）
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 專案根目錄
static BASE_DIR: LazyLock<PathBuf> = LazyLock::new(resolve_base_dir);
/// 文章目錄（對應 Python 的 ARTICLE_DIR）
static ARTICLE_DIR: LazyLock<PathBuf> = LazyLock::new(|| BASE_DIR.join("appdata").join("article"));

/// 應用程式共享狀態
#[derive(Clone)]
struct AppState {
    cache: Arc<cache::ArticleCache>,
    config: Arc<config::ServerConfig>,
    rate_limiter: Arc<RateLimiter>,
    search_index: Arc<search_index::SearchIndex>,
}

impl AppState {
    fn new(
        cache: Arc<cache::ArticleCache>,
        config: Arc<config::ServerConfig>,
        search_index: Arc<search_index::SearchIndex>,
    ) -> Self {
        let rate_limiter = Arc::new(RateLimiter::new(
            config.rate_limit.window_secs,
            config.rate_limit.max_requests,
        ));
        Self {
            cache,
            config,
            rate_limiter,
            search_index,
        }
    }
}

// ── 每 IP 滑動窗口限速器 ────────────────────────────────

/// 滑動窗口限速：記錄每個 IP 的請求時間戳，窗口內超過 `max_requests` 即拒絕。
/// 內部使用 Mutex 保護 HashMap，衝突極低（僅每次請求一次鎖）。
struct RateLimiter {
    window: Duration,
    max_requests: u32,
    hits: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl RateLimiter {
    fn new(window_secs: u64, max_requests: u32) -> Self {
        Self {
            window: Duration::from_secs(window_secs),
            max_requests,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// 嘗試放行：窗口內請求數未達上限時記錄並回傳 `true`；已達上限回傳 `false`。
    fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        let queue = hits.entry(ip).or_default();
        while let Some(&t) = queue.front() {
            if t < cutoff {
                queue.pop_front();
            } else {
                break;
            }
        }
        if queue.len() >= self.max_requests as usize {
            return false;
        }
        queue.push_back(now);
        true
    }

    /// 清除所有已記錄的 IP（供測試重置）
    #[cfg(test)]
    fn clear(&self) {
        self.hits.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

// ── 回應輔助 ──────────────────────────────────────────────

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

/// 發送 JSON 錯誤回應（與 Python `_send_error` 一致）
fn json_error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [(CONTENT_TYPE, "application/json; charset=utf-8")],
        Json(ErrorBody { error: message }),
    )
        .into_response()
}

// ── GET /api/articles ────────────────────────────────────

#[derive(Serialize)]
struct ArticlesResponse {
    articles: Vec<Article>,
}

async fn handle_articles(State(state): State<AppState>) -> Response {
    // 從快取取得文章列表（由 notify + 定時掃描保持新鮮）
    let articles = state.cache.get_articles().await;
    // 與 Python `json.dumps(..., ensure_ascii=False, indent=2)` 一致
    let body = match serde_json::to_string_pretty(&ArticlesResponse { articles }) {
        Ok(body) => body,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "內部錯誤"),
    };
    (
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

// ── GET /api/config ──────────────────────────────────────

#[derive(Serialize)]
struct ConfigResponse {
    allow_markdown_html: bool,
}

/// 回傳前端渲染所需的設定（僅暴露最小必要欄位，不洩漏埠號/限速等內部細節）
async fn handle_config(State(state): State<AppState>) -> Response {
    (
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        Json(ConfigResponse {
            allow_markdown_html: state.config.allow_markdown_html,
        }),
    )
        .into_response()
}

// ── GET /api/article?path=... ────────────────────────────

/// 同步讀取階段的錯誤分類（在 spawn_blocking 內產生，避免 async 中阻塞）
enum ReadError {
    NotFound,
    Forbidden,
    TooLarge,
    Io,
}

/// 同步：路徑正規化 + 安全檢查 + 限流讀取（在 `spawn_blocking` 執行）。
/// TOCTOU 修復：不再「先 metadata 再讀取」，而是開啟檔案後用 `take(max+1)`
/// 限制讀取長度，讀完再判斷是否超限 — 檔案在檢查與讀取之間被換成更大的檔案
/// 也無法繞過上限。
fn read_article_safe(article_dir: &PathBuf, raw_path: &str, max_file_size: u64) -> Result<String, ReadError> {
    let canonical = std::fs::canonicalize(article_dir.join(raw_path))
        .map_err(|_| ReadError::NotFound)?;
    let article_real = std::fs::canonicalize(article_dir).map_err(|_| ReadError::NotFound)?;
    // 前綴檢查（防止符號連結逃逸）
    if !canonical.starts_with(&article_real) {
        return Err(ReadError::Forbidden);
    }
    if !canonical.to_string_lossy().ends_with(".md") {
        return Err(ReadError::Forbidden);
    }
    if !canonical.is_file() {
        return Err(ReadError::NotFound);
    }
    // 限流讀取：最多讀 max_file_size + 1 位元組，超限即判 TooLarge
    let file = std::fs::File::open(&canonical).map_err(|_| ReadError::Io)?;
    let mut buf = Vec::new();
    file
        .take(max_file_size + 1)
        .read_to_end(&mut buf)
        .map_err(|_| ReadError::Io)?;
    if buf.len() as u64 > max_file_size {
        return Err(ReadError::TooLarge);
    }
    String::from_utf8(buf).map_err(|_| ReadError::Io)
}

async fn handle_article(
    State(state): State<AppState>,
    Query(params): Query<Vec<(String, String)>>,
) -> Response {
    let paths: Vec<&str> = params
        .iter()
        .filter(|(k, _)| k == "path")
        .map(|(_, v)| v.as_str())
        .collect();

    // 缺少 path 參數
    if paths.len() != 1 || paths[0].trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "缺少 path 參數");
    }
    let raw_path = paths[0].trim().to_string();

    // ── 安全檢查 1：空字節檢測 ──────────────────────
    if raw_path.contains('\0') {
        return json_error(StatusCode::BAD_REQUEST, "請求含有無效字元");
    }

    // ── 安全檢查 2：拒絕含 .. 的原始路徑 ────────────
    if raw_path.split('/').any(|seg| seg == "..") {
        return json_error(StatusCode::FORBIDDEN, "不允許的路徑");
    }

    // ── 安全檢查 3（預檢）：僅允許 .md 檔案 ─────────
    if !raw_path.ends_with(".md") {
        return json_error(StatusCode::FORBIDDEN, "僅允許讀取 .md 檔案");
    }

    // 其餘檢查（canonicalize / 讀檔）皆為同步 I/O — 移入 spawn_blocking
    let max_file_size = state.config.max_file_size as u64;
    let article_dir = ARTICLE_DIR.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        read_article_safe(&article_dir, &raw_path, max_file_size)
    })
    .await
    .unwrap_or(Err(ReadError::Io));

    match outcome {
        Ok(content) => (
            StatusCode::OK,
            [
                (CONTENT_TYPE, "text/plain; charset=utf-8"),
                (CACHE_CONTROL, "no-cache"),
            ],
            content,
        )
            .into_response(),
        Err(ReadError::NotFound) => json_error(StatusCode::NOT_FOUND, "找不到檔案"),
        Err(ReadError::Forbidden) => json_error(StatusCode::FORBIDDEN, "路徑不在允許範圍內"),
        Err(ReadError::TooLarge) => {
            json_error(StatusCode::PAYLOAD_TOO_LARGE, "檔案過大（超過設定上限）")
        }
        Err(ReadError::Io) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "讀取檔案時發生錯誤"),
    }
}

// ── GET /api/search?q=... ────────────────────────────────

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<search_index::SearchResult>,
}

async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<Vec<(String, String)>>,
) -> Response {
    let queries: Vec<&str> = params
        .iter()
        .filter(|(k, _)| k == "q")
        .map(|(_, v)| v.as_str())
        .collect();

    if queries.len() != 1 || queries[0].trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "缺少 q 參數");
    }
    let q = queries[0].trim();

    // 安全：長度限制（字元數，與 Python len() 一致）
    if q.chars().count() > 200 {
        return json_error(StatusCode::BAD_REQUEST, "查詢字串過長（上限 200 字元）");
    }

    // 安全：空字節
    if q.contains('\0') {
        return json_error(StatusCode::BAD_REQUEST, "請求含有無效字元");
    }

    // 純記憶體索引查詢（Tantivy），不觸碰檔案系統
    let results = state.search_index.search(q, state.config.search.max_results);
    (
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        Json(SearchResponse { results }),
    )
        .into_response()
}

// ── 靜態檔案與路由 ───────────────────────────────────────

/// 靜態檔案守衛（對所有請求生效，含 fallback）：
///   1. 攔截 `article/` 目錄 — 包含 URL 編碼變體（如 `/article%2F...`，
///      tower-http 的 ServeDir 會對路徑做 percent-decode，若不在路由層攔截
///      即可繞過 `/article/*path` 的字面匹配直接下載 article/ 內檔案）
///   2. 白名單限制 — 只允許前端三個檔案，避免整個專案目錄
///      （Cargo.toml、src/、server.py 等）被當作靜態檔案下載
async fn static_guard(req: Request, next: Next) -> Response {
    let decoded = percent_decode_str(req.uri().path())
        .decode_utf8()
        .unwrap_or_default()
        .into_owned();

    // 攔截 article/ 與 appdata/ 前綴（含編碼變體）
    if (decoded == "/article" || decoded.starts_with("/article/"))
        || (decoded == "/appdata" || decoded.starts_with("/appdata/"))
    {
        return json_error(StatusCode::NOT_FOUND, "找不到檔案").into_response();
    }

    // 防禦：拒絕解碼後路徑含 `..` 段的請求。
    // tower-http 的 ServeDir 亦會拒絕此類路徑（Component::ParentDir），
    // 此處為縱深防禦，不依賴 tower-http 的內部實作。
    if decoded.split('/').any(|seg| seg == "..") {
        return json_error(StatusCode::NOT_FOUND, "找不到檔案").into_response();
    }

    // API 路由放行（由對應 handler 處理）
    if decoded == "/api" || decoded.starts_with("/api/") {
        return next.run(req).await;
    }

    // 只允許前端白名單（含 vendor 本地化函式庫）
    let mut allowed = matches!(
        decoded.as_str(),
        "/" | "/index.html" | "/style.css" | "/script.js"
    );
    allowed |= matches!(
        decoded.as_str(),
        "/vendor/marked.min.js" | "/vendor/purify.min.js"
    );
    if !allowed {
        return json_error(StatusCode::NOT_FOUND, "找不到檔案").into_response();
    }

    next.run(req).await
}

/// 統一安全回應頭（對所有回應生效，含 static_guard 產生的錯誤回應）。
/// 注意：`style-src 'unsafe-inline'` 為必要放寬 — 前端大量使用 style 屬性與
/// element.style.* 設定樣式；script 僅允許同源（vendor 已本地化）。
async fn security_headers(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'self'",
        ),
    );
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    resp
}

/// API 限速（僅對 /api/* 生效）：每 IP 滑動窗口，超過閾值回 429。
/// 依賴 `into_make_service_with_connect_info` 提供的 ConnectInfo 擴展；
/// 無法取得 IP（如單元測試的 oneshot 請求）時以保留地址計數。
async fn rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let decoded = percent_decode_str(req.uri().path())
        .decode_utf8()
        .unwrap_or_default()
        .into_owned();
    let is_api = decoded == "/api" || decoded.starts_with("/api/");
    if !is_api {
        return next.run(req).await;
    }
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    if state.rate_limiter.allow(ip, Instant::now()) {
        next.run(req).await
    } else {
        json_error(StatusCode::TOO_MANY_REQUESTS, "請求過於頻繁，請稍後再試").into_response()
    }
}

fn build_app(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/articles", get(handle_articles))
        .route("/api/config", get(handle_config))
        .route("/api/article", get(handle_article))
        .route("/api/search", get(handle_search));

    // 靜態檔案從 BASE_DIR 提供（僅白名單檔案可達，見 static_guard）
    let serve_dir = ServeDir::new(&*BASE_DIR);

    // 注意順序：必須先設定 fallback_service，再上 layer，
    // 否則 layer 只包裹預設 fallback，之後替換的 fallback 會繞過中間件。
    // layer 掛載順序（後掛的在最外層）：security_headers 最外層確保
    // 所有回應（含 static_guard 的錯誤）都帶安全頭；rate_limit 依賴
    // with_state 注入的 AppState。
    Router::new()
        .merge(api)
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(static_guard))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

// ── CLI 與啟動 ───────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // 載入伺服器設定（appdata/config/server_config.toml；解析失敗即中止啟動）
    let config = Arc::new(config::ServerConfig::load(
        &config::ServerConfig::config_file_path(&BASE_DIR),
    )?);
    // 埠號優先級：命令列 > 環境變數 PORT > 設定檔 > 預設
    let port = config::resolve_port(
        args.port.as_deref(),
        env::var("PORT").ok().as_deref(),
        config.port,
    );

    // 建立文章快取：初始掃描 + notify 即時監控 + 定時掃描補漏
    let cache = Arc::new(cache::ArticleCache::new(ARTICLE_DIR.clone()));

    // 建立記憶體全文搜尋索引（Tantivy + jieba），並以初始文章列表全量建索引
    let search_index = Arc::new(search_index::SearchIndex::new()?);
    {
        let articles = cache.get_articles().await;
        let si = Arc::clone(&search_index);
        let ad = ARTICLE_DIR.clone();
        let mfs = config.max_file_size as u64;
        tokio::task::spawn_blocking(move || si.rebuild(&articles, &ad, mfs)).await?;
    }

    // 檔案變更（notify debounce / 定時掃描）→ 與文章列表共用同一訊號重建索引
    {
        let si = Arc::clone(&search_index);
        let ad = ARTICLE_DIR.clone();
        let mfs = config.max_file_size as u64;
        cache.set_refresh_hook(move |articles| {
            let si = Arc::clone(&si);
            let ad = ad.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || si.rebuild(&articles, &ad, mfs)).await;
            });
        });
    }

    cache.spawn_watcher();
    cache.spawn_periodic_scan(cache::PERIODIC_SCAN_INTERVAL);

    let app = build_app(AppState::new(cache, Arc::clone(&config), search_index));
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;

    println!("🚀 每日知識庫伺服器已啟動");
    println!("   ➜ 本機: http://localhost:{port}");
    println!("   ➜ 區域網路: http://<你的IP>:{port}");
    println!("   📁 文章目錄: {}", ARTICLE_DIR.display());
    println!("   💾 文章快取: notify 即時監控 + {} 秒定時掃描", cache::PERIODIC_SCAN_INTERVAL.as_secs());
    println!("   ⚡ 按 Ctrl+C 停止伺服器");

    // into_make_service_with_connect_info：為 rate_limit 中間件提供客戶端 IP
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// 建構測試用 AppState（快取以真實 article/ 目錄做初始掃描）
    async fn test_state() -> AppState {
        test_state_with(config::ServerConfig::default()).await
    }

    /// 以指定設定建構測試用 AppState；搜尋索引以真實文章目錄重建
    async fn test_state_with(config: config::ServerConfig) -> AppState {
        let cache = Arc::new(cache::ArticleCache::new(ARTICLE_DIR.clone()));
        let search_index = Arc::new(search_index::SearchIndex::new().unwrap());
        let articles = cache.get_articles().await;
        let si = Arc::clone(&search_index);
        let ad = ARTICLE_DIR.clone();
        let mfs = config.max_file_size as u64;
        tokio::task::spawn_blocking(move || si.rebuild(&articles, &ad, mfs))
            .await
            .unwrap();
        AppState::new(cache, Arc::new(config), search_index)
    }

    /// 發送 HTTP 請求並回傳回應
    async fn get(app: &axum::Router, uri: &str) -> Response {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        app.clone().oneshot(req).await.unwrap()
    }

    /// HTTP 層級整合測試：路徑安全、靜態白名單與編碼繞過防護
    #[tokio::test]
    async fn test_http_security_and_static() {
        let app = build_app(test_state().await);

        // 安全拒絕用例：(uri, 期望狀態碼)
        let reject_cases = [
            // 路徑穿越（原始與編碼變體）
            ("/api/article?path=../server.py", StatusCode::FORBIDDEN),
            ("/api/article?path=..%2fserver.py", StatusCode::FORBIDDEN),
            ("/api/article?path=%2e%2e%2fserver.py", StatusCode::FORBIDDEN),
            // 絕對路徑（Rust Path::join 直接替換 → 不在 article 內）
            ("/api/article?path=/etc/passwd", StatusCode::FORBIDDEN),
            // 非 .md
            ("/api/article?path=foo.txt", StatusCode::FORBIDDEN),
            // 不存在的 .md
            ("/api/article?path=no-such.md", StatusCode::NOT_FOUND),
            // 缺少參數
            ("/api/article", StatusCode::BAD_REQUEST),
            ("/api/search", StatusCode::BAD_REQUEST),
            // 空字節
            ("/api/search?q=%00", StatusCode::BAD_REQUEST),
            ("/api/article?path=a%00b.md", StatusCode::BAD_REQUEST),
            // q 超長（201 字元）
            (
                &format!("/api/search?q={}", "a".repeat(201)),
                StatusCode::BAD_REQUEST,
            ),
            // article/ 直接暴露（普通 + 編碼變體 + 帶 .. 變體）
            ("/article/foo", StatusCode::NOT_FOUND),
            ("/article", StatusCode::NOT_FOUND),
            ("/article%2Ffoo.md", StatusCode::NOT_FOUND),
            ("/article%2F..%2F..%2Fserver.py", StatusCode::NOT_FOUND),
            // appdata/（含設定檔與編碼變體）不應作為靜態檔案暴露
            ("/appdata", StatusCode::NOT_FOUND),
            ("/appdata/config/server_config.toml", StatusCode::NOT_FOUND),
            ("/appdata%2F..%2F..%2Fserver.py", StatusCode::NOT_FOUND),
            // 混用 /api/ 前綴繞過白名單（tower-http 拒絕 .. 段 + 本層防禦）
            ("/api/../Cargo.toml", StatusCode::NOT_FOUND),
            ("/api%2F..%2F..%2Fserver.py", StatusCode::NOT_FOUND),
            ("/api/../index.html", StatusCode::NOT_FOUND),
            // 專案檔案不應作為靜態檔案暴露
            ("/server.py", StatusCode::NOT_FOUND),
            ("/Cargo.toml", StatusCode::NOT_FOUND),
            ("/src/main.rs", StatusCode::NOT_FOUND),
            ("/README.md", StatusCode::NOT_FOUND),
            ("/favicon.ico", StatusCode::NOT_FOUND),
        ];

        for (uri, expected) in reject_cases {
            let req = Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), expected, "uri: {uri}");
        }

        // 靜態白名單檔案（含 vendor 本地化函式庫）
        for uri in [
            "/", "/index.html", "/style.css", "/script.js",
            "/vendor/marked.min.js", "/vendor/purify.min.js",
        ] {
            let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "uri: {uri}");
        }
    }

    /// HTTP 層級整合測試：正常 API 回應（依賴專案內的範例文章）
    #[tokio::test]
    async fn test_http_api_happy_path() {
        let app = build_app(test_state().await);

        // 讀取真實文章（ASCII 路徑）
        let req = Request::builder()
            .uri("/api/article?path=knowledges/20260730/learning-notes.md")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("# 今天的學習筆記"));

        // 中文路徑（百分號編碼）
        let req = Request::builder()
            .uri("/api/article?path=%E9%97%9C%E6%96%BC%E6%9C%AC%E7%AB%99.md")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("知識庫"));

        // 文章列表
        let req = Request::builder().uri("/api/articles").body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let articles = json["articles"].as_array().expect("articles 應為陣列");
        assert!(!articles.is_empty());
        // 有日期的文章應在無日期之後
        let first = &articles[0];
        assert!(first["date"].is_null());

        // 搜尋（檔名匹配）
        let req = Request::builder()
            .uri("/api/search?q=learning")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!json["results"].as_array().unwrap().is_empty());

        // 搜尋（無匹配）
        let req = Request::builder()
            .uri("/api/search?q=zzzz-no-such-term")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["results"].as_array().unwrap().is_empty());

        // /api/config 預設 allow_markdown_html = false
        let req = Request::builder().uri("/api/config").body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["allow_markdown_html"], serde_json::json!(false));
    }

    /// 安全回應頭：所有回應（靜態與 API）都應攜帶
    #[tokio::test]
    async fn test_security_headers_present() {
        let app = build_app(test_state().await);

        // 靜態頁面
        let resp = get(&app, "/").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let h = resp.headers();
        assert!(h.contains_key("content-security-policy"));
        assert!(h.contains_key("x-content-type-options"));
        assert!(h.contains_key("x-frame-options"));
        assert!(h.contains_key("referrer-policy"));
        assert_eq!(
            h.get("x-content-type-options").unwrap(),
            "nosniff",
        );

        // API 回應
        let resp = get(&app, "/api/articles").await;
        assert!(resp.headers().contains_key("content-security-policy"));

        // 錯誤回應（static_guard 產生的 404）也應帶安全頭
        let resp = get(&app, "/no-such-file").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(resp.headers().contains_key("content-security-policy"));
    }

    /// 限速：超過閾值的 /api/* 請求回 429；靜態檔案不受限；重置後恢復放行
    #[tokio::test]
    async fn test_rate_limit_api_only() {
        let config = config::ServerConfig {
            rate_limit: config::RateLimitConfig {
                max_requests: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let state = test_state_with(config).await;
        let app = build_app(state.clone());

        // 前 3 個 API 請求放行
        for _ in 0..3 {
            let resp = get(&app, "/api/articles").await;
            assert_eq!(resp.status(), StatusCode::OK, "限速閾值內應放行");
        }
        // 第 4 個起 429
        let resp = get(&app, "/api/articles").await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "超過閾值應 429");
        let resp = get(&app, "/api/config").await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // 靜態檔案不受限
        let resp = get(&app, "/index.html").await;
        assert_eq!(resp.status(), StatusCode::OK);

        // 重置計數後恢復放行
        state.rate_limiter.clear();
        let resp = get(&app, "/api/articles").await;
        assert_eq!(resp.status(), StatusCode::OK, "重置後應恢復放行");
    }

    /// 檔案大小上限：超過 max_file_size 的文章應回 413（TOCTOU 修復後仍生效）
    #[tokio::test]
    async fn test_article_over_max_size_413() {
        let config = config::ServerConfig {
            max_file_size: 16, // 16 位元組
            ..Default::default()
        };
        let app = build_app(test_state_with(config).await);

        let resp = get(&app, "/api/article?path=knowledges/20260730/learning-notes.md").await;
        assert_eq!(
            resp.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "超過設定上限的文章應回 413"
        );
    }
}
