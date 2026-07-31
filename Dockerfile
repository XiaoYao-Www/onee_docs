# ── 階段一：編譯 ──────────────────────────────────────────
# 建置階段使用 root 進行編譯；rust:1.88 為明確版本標籤。
# 版本依據：Cargo.lock 鎖定的 tantivy 0.26.1 其 MSRV 為 1.86，
# jieba-rs 0.10.3（傳遞依賴）為 edition 2024（需 1.85+），故選 1.88 並留緩衝。
# 若需最高可重現性，可用 `docker manifest inspect rust:1.88-slim-bookworm`
# 取得 digest 後 pin（如 `rust@sha256:…`）。
FROM rust:1.88-slim-bookworm AS build

WORKDIR /build

# 先複製清單與原始碼以善用層快取
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# 編譯 release binary
RUN cargo build --release

# ── 階段二：執行 ──────────────────────────────────────────
# bookworm-slim 為發行版標籤（持續接收安全更新）；亦可 pin 到具體日期標籤
# （如 debian:bookworm-YYYYMMDD-slim）以完全鎖定建置內容。
FROM debian:bookworm-slim

# 建立非 root 使用者（安全最佳實踐）
RUN groupadd --system --gid 1001 appgroup \
    && useradd --system --uid 1001 --gid 1001 appuser

# HEALTHCHECK 所需：curl（最小化安裝）
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 複製 binary 與前端檔案（不含 appdata/article/ 內容，由使用者 -v 掛載）
COPY --from=build /build/target/release/daily-knowledge ./
COPY index.html style.css script.js ./
COPY vendor/ ./vendor/
# 設定模板隨鏡像提供（使用者可 -v 掛載覆寫）
COPY appdata/config/server_config.toml ./appdata/config/server_config.toml

# 建立空的文章目錄供使用者掛載
RUN mkdir -p /app/appdata/article && chown -R appuser:appgroup /app/appdata

# 預設埠號（可透過 -e PORT=xxxx 覆蓋）
ENV PORT=8765
EXPOSE 8765

# 健康檢查：確認 API 可回應
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT:-8765}/api/config" || exit 1

USER appuser

CMD ["./daily-knowledge"]
