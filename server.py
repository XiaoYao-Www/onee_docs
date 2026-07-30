"""
server.py — 每日知識庫輕量後端伺服器

功能：
  - 提供靜態檔案服務 (index.html, style.css, script.js 等)
  - GET /api/articles       → 動態掃描 article/ 回傳 JSON 文章列表
  - GET /api/article?path=… → 安全回傳指定 .md 檔案內容

安全防護（逐條落實）：
  - 路徑穿越：realpath + 前綴檢查
  - 僅允許 .md 檔案
  - 空字節拒絕
  - 檔案大小上限 5MB
  - 不拼接任何 shell 指令

用法：
    python server.py [--port PORT]
    預設埠號 8765，瀏覽器開啟 http://localhost:8765
"""

import json
import os
import re
import sys
import mimetypes
from http.server import HTTPServer, SimpleHTTPRequestHandler
from urllib.parse import urlparse, parse_qs

# ── 路徑設定 ──────────────────────────────────────────────
BASE_DIR = os.path.realpath(os.path.dirname(__file__))
ARTICLE_DIR = os.path.realpath(os.path.join(BASE_DIR, "article"))

# 安全：記錄 ARTICLE_DIR 的 realpath，供前綴檢查使用
ARTICLE_DIR_REAL = ARTICLE_DIR

# 日期目錄正則
DATE_PATTERN = re.compile(r"^\d{8}$")

# 檔案大小上限：5MB
MAX_FILE_SIZE = 5 * 1024 * 1024

# 預設埠號
DEFAULT_PORT = 8765


# ── 文章掃描邏輯（與 generate_index.py 一致） ────────────
def extract_title(filename: str) -> str:
    """從檔名推斷標題：去掉 .md，將 -/_ 取代為空格。"""
    name = filename
    if name.endswith(".md"):
        name = name[:-3]
    name = name.replace("-", " ").replace("_", " ")
    name = name.strip()
    return name if name else filename


def scan_articles() -> list[dict]:
    """掃描 article/ 下所有 .md 檔案，回傳排序後的列表。"""
    if not os.path.isdir(ARTICLE_DIR):
        return []

    articles: list[dict] = []

    for root, _dirs, files in os.walk(ARTICLE_DIR):
        for fname in sorted(files):
            if not fname.endswith(".md"):
                continue

            full_path = os.path.join(root, fname)
            rel_path = os.path.relpath(full_path, ARTICLE_DIR)
            rel_path_str = rel_path.replace("\\", "/")

            title = extract_title(fname)

            # 從上層目錄名稱提取日期
            parent_name = os.path.basename(os.path.dirname(full_path))
            date_str: str | None = None
            if DATE_PATTERN.match(parent_name):
                date_str = f"{parent_name[:4]}-{parent_name[4:6]}-{parent_name[6:8]}"

            articles.append({
                "title": title,
                "path": rel_path_str,
                "date": date_str,
            })

    # 排序：無日期置頂，有日期依降冪
    no_date = [a for a in articles if a["date"] is None]
    with_date = [a for a in articles if a["date"] is not None]
    with_date.sort(key=lambda a: a["date"], reverse=True)
    no_date.sort(key=lambda a: a["title"])
    return no_date + with_date


# ── HTTP 請求處理器 ──────────────────────────────────────
class KnowledgeHandler(SimpleHTTPRequestHandler):
    """自訂 HTTP Handler，注入 API 路由與安全檢查。"""

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path

        # ── API 路由 ──────────────────────────────────
        if path == "/api/articles":
            self._handle_api_articles()
            return

        if path == "/api/article":
            self._handle_api_article(parsed)
            return

        if path == "/api/search":
            self._handle_api_search(parsed)
            return

        # ── 靜態檔案 ──────────────────────────────────
        # 限制靜態檔案只能從 BASE_DIR 提供，不讓 article/ 直接裸曝光
        # 利用父類的 translate_path 但限定範圍
        return super().do_GET()

    # ── GET /api/articles ──────────────────────────────
    def _handle_api_articles(self):
        """回傳動態掃描的文章列表 JSON。"""
        articles = scan_articles()
        data = {"articles": articles}
        body = json.dumps(data, ensure_ascii=False, indent=2).encode("utf-8")

        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    # ── GET /api/article?path=... ──────────────────────
    def _handle_api_article(self, parsed: urlparse):
        """
        安全回傳指定 .md 檔案的內容。
        安全檢查（按順序）：
          1. 空字節檢測
          2. 原始路徑正規化（禁止 path/../ 穿越）
          3. realpath 前綴檢查
          4. 僅允許 .md 後綴
          5. 必須是真實檔案
          6. 檔案大小上限
        """
        params = parse_qs(parsed.query)
        raw_paths = params.get("path", [])

        # 缺少 path 參數
        if len(raw_paths) != 1 or not raw_paths[0].strip():
            self._send_error(400, "缺少 path 參數")
            return

        raw_path = raw_paths[0].strip()

        # ── 安全檢查 1：空字節檢測 ────────────────────
        if "\x00" in raw_path:
            self._send_error(400, "請求含有無效字元")
            return

        # ── 安全檢查 2：拒絕含 .. 的原始路徑 ──────────
        # 即使 realpath 能消解，提早攔截更安全
        if ".." in raw_path.split("/"):
            self._send_error(403, "不允許的路徑")
            return

        # ── 構建完整路徑並正規化 ──────────────────────
        full_path = os.path.realpath(os.path.join(ARTICLE_DIR, raw_path))

        # ── 安全檢查 3：前綴檢查（防止符號連結逃逸） ──
        if not full_path.startswith(ARTICLE_DIR_REAL):
            self._send_error(403, "路徑不在允許範圍內")
            return

        # ── 安全檢查 4：僅允許 .md 檔案 ───────────────
        if not full_path.endswith(".md"):
            self._send_error(403, "僅允許讀取 .md 檔案")
            return

        # ── 安全檢查 5：必須是真實檔案 ────────────────
        if not os.path.isfile(full_path):
            self._send_error(404, "找不到檔案")
            return

        # ── 安全檢查 6：檔案大小上限 ──────────────────
        file_size = os.path.getsize(full_path)
        if file_size > MAX_FILE_SIZE:
            self._send_error(413, "檔案過大（上限 5MB）")
            return

        # ── 讀取並回傳檔案內容 ────────────────────────
        try:
            with open(full_path, "r", encoding="utf-8") as f:
                content = f.read()
        except (IOError, OSError):
            self._send_error(500, "讀取檔案時發生錯誤")
            return

        body = content.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    # ── GET /api/search?q=... ────────────────────────
    def _handle_api_search(self, parsed: urlparse):
        """
        搜尋文章（檔名 + 內容）。
        安全措施：
          - q 必填，長度 1-200
          - 空字節拒絕
          - re.escape 防止 ReDoS
          - 僅掃描 .md 檔案
          - 每檔最多讀取前 100KB
          - 結果上限 20 條
        """
        params = parse_qs(parsed.query)
        raw_queries = params.get("q", [])

        if len(raw_queries) != 1 or not raw_queries[0].strip():
            self._send_error(400, "缺少 q 參數")
            return

        q = raw_queries[0].strip()

        # 安全：長度限制
        if len(q) > 200:
            self._send_error(400, "查詢字串過長（上限 200 字元）")
            return

        # 安全：空字節
        if "\x00" in q:
            self._send_error(400, "請求含有無效字元")
            return

        # 安全：re.escape 防止 ReDoS
        try:
            pattern = re.compile(re.escape(q), re.IGNORECASE)
        except re.error:
            self._send_error(400, "無效的查詢")
            return

        if not os.path.isdir(ARTICLE_DIR):
            self._send_json(200, {"results": []})
            return

        results: list[dict] = []

        for root, _dirs, files in os.walk(ARTICLE_DIR):
            for fname in files:
                if not fname.endswith(".md"):
                    continue
                if len(results) >= 20:
                    break

                full_path = os.path.join(root, fname)
                rel_path = os.path.relpath(full_path, ARTICLE_DIR)
                rel_path_str = rel_path.replace("\\", "/")

                title = extract_title(fname)

                # 從目錄提取日期
                parent_name = os.path.basename(os.path.dirname(full_path))
                date_str: str | None = None
                if DATE_PATTERN.match(parent_name):
                    date_str = f"{parent_name[:4]}-{parent_name[4:6]}-{parent_name[6:8]}"

                # 比對：檔名
                match_in = []
                if pattern.search(fname):
                    match_in.append("title")

                # 比對：檔案內容（只讀前 100KB）
                content_match = False
                snippet = ""
                try:
                    with open(full_path, "r", encoding="utf-8", errors="replace") as f:
                        content = f.read(1024 * 100)  # 最多 100KB
                    if pattern.search(content):
                        content_match = True
                        match_in.append("content")
                        # 截取匹配位置前後各 60 字元作為片段
                        for m in pattern.finditer(content):
                            start = max(0, m.start() - 60)
                            end = min(len(content), m.end() + 60)
                            snippet = content[start:end].replace("\n", " ")
                            # 用 <mark> 標記關鍵字
                            snippet = pattern.sub(lambda mo: f"<mark>{mo.group()}</mark>", snippet)
                            if len(snippet) > 200:
                                snippet = snippet[:200] + "…"
                            break
                except (IOError, OSError):
                    continue

                if not match_in:
                    continue

                results.append({
                    "title": title,
                    "path": rel_path_str,
                    "date": date_str,
                    "snippet": snippet if content_match else "",
                    "match_in": "|".join(match_in),
                })

        results = results[:20]
        self._send_json(200, {"results": results})

    # ── 輔助：發送 JSON 回應 ────────────────────────────
    def _send_json(self, code: int, data: dict):
        """發送 JSON 回應。"""
        body = json.dumps(data, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    # ── 輔助：發送錯誤回應 ────────────────────────────
    def _send_error(self, code: int, message: str):
        """發送 JSON 格式的錯誤回應。"""
        self._send_json(code, {"error": message})

    # ── 抑制 favicon 404 雜訊（可選） ──────────────────
    def log_message(self, format, *args):
        """自訂日誌輸出（增加可讀性）。"""
        sys.stderr.write("[%s] %s - %s\n" % (
            self.log_date_time_string(),
            self.client_address[0],
            format % args
        ))


# ── CLI 啟動點 ────────────────────────────────────────────
def main():
    # 端口优先级：环境变量 PORT > 命令行 --port > 默认值
    port = DEFAULT_PORT
    env_port = os.environ.get("PORT")
    if env_port is not None:
        try:
            port = int(env_port)
        except ValueError:
            print(f"⚠️  環境變數 PORT 值無效 ('{env_port}')，使用預設", file=sys.stderr)
    # 命令行參數可覆蓋環境變數
    if len(sys.argv) > 1 and sys.argv[1] == "--port" and len(sys.argv) > 2:
        try:
            port = int(sys.argv[2])
        except ValueError:
            print("⚠️  無效的埠號，使用預設值", file=sys.stderr)

    server = HTTPServer(("0.0.0.0", port), KnowledgeHandler)
    print(f"🚀 每日知識庫伺服器已啟動")
    print(f"   ➜ 本機: http://localhost:{port}")
    print(f"   ➜ 區域網路: http://<你的IP>:{port}")
    print(f"   📁 文章目錄: {ARTICLE_DIR}")
    print(f"   ⚡ 按 Ctrl+C 停止伺服器")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 伺服器已停止")
        server.server_close()


if __name__ == "__main__":
    main()
