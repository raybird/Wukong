# Changelog

本專案的所有重要變更都會記錄在此檔。

格式依循 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版本號採用 [Semantic Versioning](https://semver.org/lang/zh-TW/)（實際發佈 tag 為 `v0.16.x`）。

> 本 CHANGELOG 自 `v0.16.35` 起維護；更早的版本紀錄請見
> [GitHub Releases](https://github.com/raybird/Wukong/releases) 與 `git log`。
> 維護方式：每次發佈前，把 `Unreleased` 區塊的條目整理為新版本區塊並標上發佈日期。

## [Unreleased]

### Changed

- Release workflow 改為「並行建置 → 單一 `publish` job 統一上傳」，消除多個 matrix
  job 並行寫入同一 GitHub Release 造成的資產上傳競態。
- 停止發佈 Intel Mac（`x86_64-apple-darwin`）預建二進位，改為僅發佈 Apple Silicon
  （`aarch64-apple-darwin`）。`scripts/install.sh` 於 Intel Mac 改給 Docker／
  原始碼建置指引，而非下載不存在的資產。

## [0.16.38] - 2026-07-07

### Changed

- 純結構重構（無行為變更）：拆分兩個過大檔案，以測試守門。
  - `wukong-gateway/src/opencode_server.rs`（1401 → 640 行）抽出
    `opencode_server/sse.rs` 與 `opencode_server/event_map.rs`。
  - `wukong-web/src/lib.rs`（3772 → 2955 行）抽出 `static_assets.rs` 與 `chat_api.rs`。

## [0.16.37] - 2026-07-07

### Added

- 新增 `CHANGELOG.md`（Keep a Changelog）。
- `wukong-web` 新增免認證 `/healthz` liveness 端點與 docker-compose healthcheck。
- `Dockerfile` 新增 `ARG OPENCODE_VERSION`，可於 build 時 pin opencode-ai 版本。

### Changed

- `Dockerfile` 的 `ARG VERSION` 預設更新為 `v0.16.36`。

### Documentation

- 文件化 `opencode serve` backend 刻意不串流回答文字（收尾以 `list_messages`
  一次取回）之設計，與 CLI backend 的差異。

## [0.16.36] - 2026-07-07

### Changed

- 進入點膠水碼下沉至 `wukong-runtime`：`now_unix`、`upload_root`、`default_db_url`、
  `db_url_from_env`、`agent_command_from_env` 與 memory bootstrap 各只剩一份實作。
- 新增 `wukong-scheduler::run_claimed_job` 統一 CLI 與 daemon 的排程執行編排
  （`start_run → execute → finish_run → complete_claimed_job`），lease 語意不變。
- CLI `run_one` 改用 `render::StreamRenderer`；REPL 逐回合重載 settings，
  讓 `/set_models` 於下一題即生效。

### Removed

- 移除死碼 `wukong-gateway/src/pipeline.rs`（無 production 呼叫者，已被
  `wukong_runtime::run_turn` 取代）。

## [0.16.35] - 2026-07-07

### Added

- CI workflow（`fmt` / `clippy -D warnings` / `test`，push main 與 PR 觸發）；
  `Cargo.lock` 納入版控，release 建置全面 `--locked`。

### Fixed

- **render**：修正 `split_chunks` 的 CJK 分塊 panic 與 `<pre>` 標籤不平衡
  導致 Telegram 400；連結／圖片 URL scheme allowlist，擋掉 `javascript:` XSS。
- **gateway**：CLI backend 實際傳遞 `--agent` 旗標，修正規劃棒被靜默丟棄。
- **telegram**：傳輸層辨識 `ok:false` 錯誤並退避；token 輪替保留 offset 游標。
- **memory**：embedding 改走 `spawn_blocking` 避免阻塞 async runtime；
  修正向量召回被 recency 截斷；`touch`／`consolidate`／`delete` 改原子 `IN(...)` 更新；
  SQLite `busy_timeout` 降低鎖競爭。

### Security

- **web / memoryd**：認證改用 middleware 閘門並支援 `Authorization: Bearer`；
  不安全綁定（`0.0.0.0` + 空 token）啟動即拒絕（fail-closed，可用
  `WUKONG_WEB_ALLOW_INSECURE=1` 覆寫）；Telegram callback 加白名單檢查。

[Unreleased]: https://github.com/raybird/Wukong/compare/v0.16.38...HEAD
[0.16.38]: https://github.com/raybird/Wukong/compare/v0.16.37...v0.16.38
[0.16.37]: https://github.com/raybird/Wukong/compare/v0.16.36...v0.16.37
[0.16.36]: https://github.com/raybird/Wukong/compare/v0.16.35...v0.16.36
[0.16.35]: https://github.com/raybird/Wukong/releases/tag/v0.16.35
