# 每日知識庫 📚

輕量級 Markdown 知識部落格 — 每日記錄學習心得，自動掃描文章，無需資料庫。

## ✨ 特色

- 🖊️ **純 Markdown** — 用 `.md` 檔案寫文章，無需後台編輯器
- 📁 **自動掃描** — 啟動伺服器後自動辨識 `article/` 目錄下所有 `.md` 檔案
- 📅 **日期分組** — 支援 `article/knowledges/YYYYMMDD/` 目錄結構，按日期歸類
- 🔗 **分享連結** — 每篇文章都有專屬 URL，可直接分享
- 🐳 **Docker 部署** — 一鍵容器化，支援掛載外部文章目錄
- 🔒 **安全設計** — 路徑穿越防護、非 root 執行、檔案大小限制

## 🚀 快速開始

### 本地執行

```bash
# 預設埠號 8765
python server.py

# 指定埠號（命令列參數）
python server.py --port 3000

# 指定埠號（環境變數，優先級低於命令列參數）
PORT=3000 python server.py
```

開啟瀏覽器訪問 `http://localhost:8765`（或你指定的埠號）。

### Docker 部署

```bash
# 基本執行（掛載你的文章目錄）
docker run -d \
  --name daily-knowledge \
  -p 8765:8765 \
  -v /你的文章路徑:/app/article \
  ghcr.io/你的帳號/倉庫:latest

# 指定其他埠號
docker run -d \
  --name daily-knowledge \
  -p 8080:8080 \
  -e PORT=8080 \
  -v /你的文章路徑:/app/article \
  ghcr.io/你的帳號/倉庫:latest
```

> ⚠️ 注意：容器內的 `article/` 目錄為空，請務必透過 `-v` 掛載你的 `.md` 文章目錄。

## 📂 文章目錄結構

```
article/
├── 關於本站.md              ← 無日期的文章（置頂）
└── knowledges/
    └── 20260730/            ← 日期目錄（YYYYMMDD）
        └── learning-notes.md  ← 檔案名稱自動轉為標題
```

**命名規則**：
- 檔名中的 `-` 和 `_` 會自動取代為空格（`learning-notes.md` → `learning notes`）
- 父目錄名稱為 8 位數字時，自動提取為日期（`20260730` → `2026-07-30`）
- 無日期目錄的文章會排在列表最上方（置頂）

## 📡 API 端點

| 端點 | 說明 |
|------|------|
| `GET /api/articles` | 回傳文章列表 JSON（標題、路徑、日期） |
| `GET /api/article?path=xxx.md` | 回傳指定 `.md` 檔案內容（純文字） |
| `GET /` 靜態檔案 | `index.html`、`style.css`、`script.js` |

## 🛠️ 開發

本專案使用純 Python 標準庫，無需安裝任何第三方套件。

```bash
# 啟動開發伺服器
python server.py

# 重新產生靜態索引（備用，一般不需要）
python scripts/generate_index.py
```

### 技術棧

- **後端**：Python 3 `http.server`（標準庫）
- **前端**：原生 JavaScript + CSS（無框架）
- **Markdown 渲染**：Marked.js（CDN）
- **容器**：Docker（python:3.12-slim）

## 🔒 安全

- 路徑穿越防護：`realpath` + 前綴檢查
- 僅允許讀取 `.md` 檔案
- 空字節注入拒絕
- 檔案大小上限 5MB
- Docker 容器以非 root 使用者執行
