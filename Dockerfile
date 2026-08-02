# syntax=docker/dockerfile:1.7


############################################################
# Stage 1: Planner
# 產生 dependency recipe，讓 Rust crate 可以被快取
############################################################

FROM lukemathwalker/cargo-chef:latest-rust-1.88 AS chef

WORKDIR /build


FROM chef AS planner

COPY Cargo.toml Cargo.lock ./

COPY src ./src

RUN cargo chef prepare \
    --recipe-path recipe.json



############################################################
# Stage 2: Builder
# 編譯 Rust binary
############################################################

FROM chef AS builder

WORKDIR /build


COPY --from=planner /build/recipe.json recipe.json


# 編譯 dependencies
# 只要 Cargo.toml / Cargo.lock 沒變
# 這層可以直接使用 cache
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook \
    --release \
    --recipe-path recipe.json



# 複製完整專案
COPY . .


# 編譯正式版本
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build \
    --release \
    --locked



############################################################
# Stage 3: Runtime
# 最小執行環境
############################################################

FROM debian:bookworm-slim AS runtime


LABEL org.opencontainers.image.title="onee-docs"


# 建立非 root 使用者
RUN groupadd \
        --system \
        --gid 1001 \
        appgroup \
    && useradd \
        --system \
        --uid 1001 \
        --gid 1001 \
        appuser


WORKDIR /app


# Rust binary
COPY --from=builder \
    /build/target/release/onee-docs \
    ./onee-docs


# 前端資源
COPY index.html ./
COPY style.css ./
COPY script.js ./

COPY vendor ./vendor


# 資料目錄：appdata/（文章 + 設定）由使用者掛載/創建，不隨鏡像附帶。
# 未掛載時伺服器以內建預設值啟動，文章目錄為空。
RUN mkdir -p \
        /app/appdata/article \
    && mkdir -p \
        /app/appdata/config \
    && chown -R \
        appuser:appgroup \
        /app


ENV PORT=8765


EXPOSE 8765


USER appuser


CMD ["./onee-docs"]