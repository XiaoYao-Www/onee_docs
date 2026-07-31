# 每日知識庫 📚

輕量級 Markdown 知識部落格 — 每日記錄學習心得，自動掃描文章，無需資料庫。

## ✨ 特色

- 🖊️ **純 Markdown** — 用 `.md` 檔案寫文章，無需後台編輯器
- 📁 **自動掃描 + 快取** — 文章列表以記憶體快取提供，透過檔案監控（notify）即時更新，並以定時掃描補漏
- 📅 **日期分組** — 支援 `appdata/article/knowledges/YYYYMMDD/` 目錄結構，按日期歸類
- 🔗 **分享連結** — 每篇文章都有專屬 URL，可直接分享
- 🔍 **全文搜尋** — Tantivy 全文檢索 + jieba 中文分詞（BM25 相關性排序、結果高亮）
- ⚙️ **設定檔驅動** — `appdata/config/server_config.toml` 集中管理伺服器行為
- 🐳 **Docker 部署** — 一鍵容器化，支援掛載外部文章目錄
- 🔒 **安全設計** — 路徑穿越防護、Markdown HTML 預設禁用（DOMPurify 淨化）、非 root 執行、檔案大小限制、API 限速

## 📂 目錄結構

```
daily_knowledge/
├── appdata/                     ← 資料目錄（文章 + 設定）
│   ├── article/                 ← 文章目錄（私有內容，預設不隨倉庫提交）
│   │   ├── 關於本站.md
│   │   └── knowledges/
│   │       └── 20260730/        ← 日期目錄（YYYYMMDD）
│   │           └── learning-notes.md
│   └── config/
│       └── server_config.toml   ← 伺服器設定（含註解的預設模板）
├── vendor/                      ← 本地化前端函式庫（marked + DOMPurify，固定版本）
├── src/                         ← Rust 後端原始碼
├── index.html / style.css / script.js
└── Dockerfile
```

**文章命名規則**：
- 檔名中的 `-` 和 `_` 會自動取代為空格（`learning-notes.md` → `learning notes`）
- 父目錄名稱為 8 位數字時，自動提取為日期（`20260730` → `2026-07-30`）
- 無日期目錄的文章會排在列表最上方（置頂）

## 🚀 快速開始

### 本地執行（需要 Rust toolchain）

```bash
# 預設埠號 8765
cargo run

# 指定埠號（命令列參數，優先級最高）
cargo run -- --port 3000

# 指定埠號（環境變數）
PORT=3000 cargo run

# 編譯 release 後直接執行
cargo build --release
./target/release/daily-knowledge
```

開啟瀏覽器訪問 `http://localhost:8765`（或你指定的埠號）。

### Docker 部署

```bash
# 基本執行（掛載你的文章目錄）
docker run -d \
  --name daily-knowledge \
  -p 8765:8765 \
  -v /你的文章路徑:/app/appdata/article \
  ghcr.io/你的帳號/倉庫:latest
```

> ⚠️ 容器內的 `appdata/article/` 目錄為空，請務必透過 `-v` 掛載你的 `.md` 文章目錄。容器以 UID 1001（`appuser`）執行，若掛載後無法讀取文章，請確保宿主目錄對 UID 1001 具備讀取權限（如 `chmod -R o+r /你的文章路徑`）。

## ⚙️ 設定檔

設定檔位於 `appdata/config/server_config.toml`，為**選配**：檔案不存在或欄位省略時使用內建預設值；語法錯誤會導致伺服器拒絕啟動。

| 欄位 | 預設值 | 說明 |
|------|--------|------|
| `port` | 8765 | 監聽埠號（優先級：命令列 `--port` > 環境變數 `PORT` > 設定檔 > 預設） |
| `allow_markdown_html` | `false` | **是否允許 Markdown 內嵌 HTML 原樣渲染**（見下方風險說明） |
| `max_file_size` | 5242880 (5MB) | 單篇文章讀取上限（位元組） |
| `[rate_limit] window_secs` | 60 | API 限速窗口長度（秒） |
| `[rate_limit] max_requests` | 300 | 窗口內每 IP 允許的最大 API 請求數 |
| `[search] max_results` | 20 | 搜尋結果上限 |

### ⚠️ `allow_markdown_html` 風險說明

- `false`（**預設，安全**）：文章中的 `<script>`、`<img onerror>`、`javascript:` 連結等活動 HTML 會被 DOMPurify 清除，只允許標準 Markdown 語法。**建議保持預設**。
- `true`：文章內嵌的 HTML 原樣渲染（可用於嵌入 iframe、自訂表格樣式等），但**等同信任文章作者可對所有讀者執行任意腳本** — 僅在文章來源完全可信（僅自己維護）時才考慮開啟。

> 無論設定為何，搜尋結果片段都會被淨化（僅保留 `<mark>` 高亮），且 `script-src 'self'` 的 CSP 會阻止外部腳本載入。

## 📡 API 端點

| 端點 | 說明 |
|------|------|
| `GET /api/articles` | 回傳文章列表 JSON（標題、路徑、日期） |
| `GET /api/article?path=xxx.md` | 回傳指定 `.md` 檔案內容（純文字） |
| `GET /api/search?q=關鍵字` | 搜尋文章（檔名 + 內容），Tantivy + jieba 分詞、BM25 相關性排序，結果上限可設定 |
| `GET /api/config` | 回傳前端渲染所需設定（目前僅 `allow_markdown_html`） |
| `GET /` 靜態檔案 | `index.html`、`style.css`、`script.js`、`vendor/*` |

## 🛠️ 開發

```bash
# 啟動開發伺服器
cargo run

# 執行測試（含路徑安全、XSS 淨化、限速、安全頭等整合測試）
cargo test

# Lint
cargo clippy

# 依賴漏洞掃描（需安裝 cargo-audit：cargo install cargo-audit）
cargo audit
```

### 技術棧

- **後端**：Rust + [axum](https://github.com/tokio-rs/axum) + tokio（`src/main.rs`、`src/articles.rs`、`src/cache.rs`、`src/config.rs`、`src/search_index.rs`），檔案監控使用 [notify](https://github.com/notify-rs/notify)
- **全文搜尋**：[tantivy](https://github.com/quickwit-oss/tantivy) 記憶體索引 + [tantivy-jieba](https://github.com/jiegec/tantivy-jieba)（jieba-rs 中文分詞）；索引由檔案變更訊號（notify + 定時掃描）驅動自動重建，搜尋請求不觸碰檔案系統
- **前端**：原生 JavaScript + CSS（無框架）
- **Markdown 渲染**：Marked.js + DOMPurify（均固定版本本地化於 `vendor/`，無第三方 CDN 依賴）
- **容器**：Docker（多階段構建：`rust:1.80-slim-bookworm` 編譯 → `debian:bookworm-slim` 執行）

## 🔒 安全設計

- **路徑穿越防護**：`canonicalize` + 元件級前綴檢查（`Path::starts_with`）+ `..` 段預檢 + 空字節拒絕，僅允許 `.md` 檔案
- **XSS 防護**：Markdown HTML 預設由 DOMPurify 淨化；所有前端動態內容以 `textContent` 呈現或白名單淨化
- **供應鏈安全**：前端函式庫本地化並固定版本；CSP `script-src 'self'` 阻止外部腳本
- **安全回應頭**：`Content-Security-Policy`、`X-Content-Type-Options: nosniff`、`X-Frame-Options: SAMEORIGIN`、`Referrer-Policy: no-referrer`
- **DoS 防護**：檔案大小上限（`take` 限流讀取，無 TOCTOU 窗口）、API 每 IP 滑動窗口限速、搜尋與索引重建的同步 I/O 置於 `spawn_blocking`、搜尋請求本身為純記憶體查詢
- **Docker**：多階段構建、非 root 使用者執行、`appdata/` 僅掛載文章目錄

### 部署安全建議

- 伺服器預設監聽 `0.0.0.0`（供區域網路存取）。**若部署到公網**，強烈建議：
  - 放在反向代理（nginx / caddy / Cloudflare Tunnel 等）之後並啟用 **TLS**（HTTPS）
  - 反向代理層可再加請求體大小與連線數限制
- 文章內容視為**站點信任邊界**：`allow_markdown_html` 保持 `false`，除非你完全信任所有文章作者
- 定期執行 `cargo audit` 檢查依賴漏洞（CI 已包含此步驟）
