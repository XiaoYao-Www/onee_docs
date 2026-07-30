"""
generate_index.py - 掃描 article/ 目錄下的所有 .md 檔案，
                   生成 article/index.json 供前端動態載入。

用法：
    python scripts/generate_index.py

輸出：
    article/index.json
"""

import json
import os
import re
from pathlib import Path

# 專案根目錄（腳本在 scripts/ 下，上層即根目錄）
ROOT = Path(__file__).resolve().parent.parent
ARTICLE_DIR = ROOT / "article"
INDEX_FILE = ARTICLE_DIR / "index.json"

# 日期目錄的正則模式：knowledges/YYYYMMDD/
DATE_PATTERN = re.compile(r"^\d{8}$")


def extract_title(filename: str) -> str:
    """從檔名推斷標題：去掉 .md，取代 -/_ 為空格。"""
    name = filename
    if name.endswith(".md"):
        name = name[:-3]
    # 取代分隔符
    name = name.replace("-", " ").replace("_", " ")
    # 去除前後空白
    name = name.strip()
    return name if name else filename


def scan_articles() -> list[dict]:
    """掃描 article/ 下所有 .md 檔案，回傳文章列表。"""
    if not ARTICLE_DIR.exists():
        print(f"❌ 找不到 article 目錄：{ARTICLE_DIR}")
        return []

    articles: list[dict] = []

    for md_file in sorted(ARTICLE_DIR.rglob("*.md")):
        # 跳過 index.json 本身（不會是 .md 但防呆）
        # 計算相對路徑（相對於 article/）
        rel_path = md_file.relative_to(ARTICLE_DIR)
        rel_path_str = str(rel_path).replace("\\", "/")  # 統一正斜線

        filename = md_file.name
        title = extract_title(filename)

        # 嘗試從上層目錄提取日期
        date_str: str | None = None
        parent_name = md_file.parent.name
        if DATE_PATTERN.match(parent_name):
            y = parent_name[:4]
            m = parent_name[4:6]
            d = parent_name[6:8]
            date_str = f"{y}-{m}-{d}"

        articles.append({
            "title": title,
            "path": rel_path_str,
            "date": date_str,
        })

    # 排序：無日期（如「關於本站」）排最前面；有日期的依日期降冪（最新在前）
    no_date = [a for a in articles if a["date"] is None]
    with_date = [a for a in articles if a["date"] is not None]
    # 日期降冪
    with_date.sort(key=lambda a: a["date"], reverse=True)
    # 無日期依標題排序
    no_date.sort(key=lambda a: a["title"])
    articles = no_date + with_date

    return articles


def main():
    print("🔍 掃描 article/ 目錄中的 .md 檔案...")
    articles = scan_articles()

    if not articles:
        print("⚠️  未找到任何 .md 檔案，仍將寫入空的 index.json。")
    else:
        print(f"✅ 找到 {len(articles)} 篇文章")

    index_data = {"articles": articles}

    # 寫入 index.json
    INDEX_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(INDEX_FILE, "w", encoding="utf-8") as f:
        json.dump(index_data, f, ensure_ascii=False, indent=2)

    print(f"📄 已寫入 {INDEX_FILE}")
    for art in articles:
        date_tag = f"[{art['date']}] " if art["date"] else ""
        print(f"   • {date_tag}{art['title']} → {art['path']}")


if __name__ == "__main__":
    main()
