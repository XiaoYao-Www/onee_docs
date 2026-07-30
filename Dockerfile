FROM python:3.12-slim

# 建立非 root 使用者（安全最佳實踐）
RUN addgroup --system --gid 1001 appgroup \
    && adduser --system --uid 1001 --gid 1001 appuser

WORKDIR /app

# 只複製應用程式檔案（不包含 article/ 內容）
# article/ 應由使用者透過 -v 掛載
COPY server.py index.html style.css script.js ./

# 建立空的 article/ 目錄供使用者掛載
RUN mkdir -p /app/article && chown appuser:appgroup /app/article

# 預設埠號（可透過 -e PORT=xxxx 覆蓋）
ENV PORT=8765
EXPOSE 8765

USER appuser

CMD ["python", "server.py"]
