# Task Implementation

> 來源計畫：`2026-07-08-web-allow-insecure-upgrade-fix.md`
> 拆解文件：`2026-07-08-web-allow-insecure-upgrade-fix-decomposition.md`
> 執行日期：2026-07-08
> 執行範圍：Decomposition Phase 1～4 全數完成（原計畫 Phase 0～3）

## Phase

- Phase 1 — 止血：升級資訊與範本補救（Task 1.1、1.2、1.3、1.4）
- Phase 2 — 治本：安全邊界搬到 host port mapping（Task 2.1、2.2、2.3）
- Phase 3 — 體驗：fail-crash 改 fail-visible（Task 3.1、3.2、3.3）
- Phase 4 — 流程：release 檢查防再犯（Task 4.1）

## Summary

修復 v0.17.0 引入的 Web Console fail-closed 守門在 Docker Compose 預設下造成的無限重啟問題。核心決策：**把安全邊界從 app-level token 檢查搬到 host 端 port mapping**。容器內固定綁 `0.0.0.0:8787`（Docker 必然）、compose 明示 `WUKONG_WEB_ALLOW_INSECURE=1`，實際暴露面改由新變數 `WUKONG_WEB_BIND`（預設 `127.0.0.1`）控制。結果是預設「localhost 直接可用、外部連不到」，比原本「全介面暴露靠 token 擋」更安全，且沿用舊 `.env` 升級即可直接使用。裸跑（非 Docker）的 fail-closed 行為完全不變。

同時順手修掉 `WUKONG_WEB_PORT` 一變數兩用造成的埠不一致 bug（覆寫時 host 埠與容器埠對不上）。

## Implemented Changes

**Phase 2 — compose 治本（Task 2.1）**
- `docker-compose.yml` wukong-web：ports 改 `"${WUKONG_WEB_BIND:-127.0.0.1}:${WUKONG_WEB_PORT:-8787}:8787"`。
- 移除 environment 中的 `WUKONG_WEB_PORT` 傳遞（容器內固定聽 8787）；`WUKONG_WEB_HOST` 固定為 `0.0.0.0`（port mapping 必要條件，移除 footgun）。
- `WUKONG_WEB_ALLOW_INSECURE` 預設改 `${WUKONG_WEB_ALLOW_INSECURE:-1}`，附 SECURITY 註解說明邊界改由 host mapping 控制。
- healthcheck 目標固定為容器內 `http://localhost:8787/healthz`（不再受 `WUKONG_WEB_PORT` 影響）。

**Phase 2 — 驗證（Task 2.2）**
- `docker compose config` 三情境渲染皆正確：(a) 預設 `host_ip: 127.0.0.1` / target 8787；(b) `WUKONG_WEB_PORT=9000` → published 9000 → target 8787（bug 已修）；(c) `WUKONG_WEB_BIND=0.0.0.0` → host_ip 0.0.0.0。
- 真實 binary 端到端驗證（免建 image）：容器等效 env（`0.0.0.0`＋空 token＋`ALLOW_INSECURE=1`）→ app 正常啟動、`/healthz` 回 200；反向（無 `ALLOW_INSECURE`）→ exit 1 拒絕啟動，fail-closed 維持。
- 既有單元測試 `refuses_insecure_public_bind_only_when_unsafe` 綠燈（`should_refuse_insecure_start` 矩陣不變）。

**Phase 1 — 文件與範本（Task 1.1／1.3／1.4，並採 Task 2.3 新語境一次到位）**
- `CHANGELOG.md`：0.17.0 區塊頂部新增「⚠️ 升級注意（Breaking）」；Unreleased 記錄 compose 預設恢復開箱即用、埠 bug 修復、新增 `WUKONG_WEB_BIND`。
- GitHub Release v0.17.0 notes：原為空，補上升級注意置頂＋版本重點＋CHANGELOG 連結（Task 1.2，`gh release edit`）。
- `.env.example`：Web Console 區塊改為新語境（預設僅本機、`WUKONG_WEB_BIND`、對外開放需搭 token）。
- `docs/docker.md`：快速開始補預設語境與診斷提示（`docker compose logs/ps`）；環境變數表新增 `WUKONG_WEB_BIND`、更新 `WUKONG_WEB_PORT`／`WUKONG_WEB_TOKEN`／`WUKONG_WEB_ALLOW_INSECURE` 說明。
- `docs/entrypoints.md` 經核對維持原樣（描述裸跑進入點，`WUKONG_WEB_HOST/PORT` 語意仍正確；`WUKONG_WEB_BIND` 為 compose-only 概念不適用）。

**Phase 3 — fail-crash 改 fail-visible（Task 3.1／3.2／3.3）**
- `crates/wukong-web/src/lib.rs`：新增 `build_misconfigured_router()`（無 state、fallback 對所有路徑回 503）＋ `serve_misconfigured_page` handler ＋ `MISCONFIGURED_HTML` 自包含說明頁（含兩種解法與 `docs/docker.md` 連結、theme-aware）。新增測試 `misconfigured_router_returns_503_with_guidance_on_all_paths`。
- `crates/wukong-web/src/main.rs`：守門分支不再 `exit(1)`，改呼叫新 helper `serve_misconfigured(&host, &port)` 綁定原位址埠常駐降級 router；正常路徑不變。
- `docs/docker.md`／`CHANGELOG.md`：記載降級模式行為（所有請求含 `/healthz` 回 503、healthcheck unhealthy、不再 crash loop）。

**Phase 4 — release 流程防再犯（Task 4.1）**
- `.claude/skills/wukong-release/SKILL.md`：Verify Candidate 段新增「Breaking env-var sync check」小節，列出四同步點（`.env.example`／`docker-compose.yml`／`docs/docker.md`／CHANGELOG+Release notes），並附 v0.17.0 事故案例連結。

## 關鍵設計決策

1. **`WUKONG_WEB_HOST` 由可覆寫改為容器內固定 `0.0.0.0`**：容器內必綁全介面才能讓 port mapping 通；保留覆寫反而是 footgun（使用者設 `127.0.0.1` 會讓 mapping 失效）。裸跑路徑不受影響（app 預設仍 `127.0.0.1`）。
2. **fail-visible 取代 fail-crash**：守門觸發時綁定原位址埠、對所有請求（含 `/healthz`）回 503 說明頁，而非 `exit(1)`。解決 `restart: unless-stopped` 下的隱形重啟迴圈；仍是 fail-closed（無 state、不掛功能路由）。Phase 3 完成後，Phase 2 遺留的「對外漏設 token」殘留風險已由此頁面 fail-visible 化。

## 驗證

- `cargo test --workspace`：全綠（wukong-web 75 tests，含新測試）。`cargo clippy -p wukong-web --all-targets -- -D warnings` 無警告，`cargo fmt --check` 通過。
- compose：`docker compose config` 三情境（預設 loopback／`WUKONG_WEB_PORT=9000`／`WUKONG_WEB_BIND=0.0.0.0`）渲染皆正確。
- 真實 binary 端到端（免建 image）：
  - 容器預設 env（`0.0.0.0`＋空 token＋`ALLOW_INSECURE=1`）→ 正常啟動、`/healthz` 200。
  - 誤配置（`0.0.0.0`＋空 token＋無 `ALLOW_INSECURE`）→ **不再 exit**，進程存活、`/`／`/healthz`／任意 API 皆 503、說明頁含兩種解法與文件連結。
  - 正確配置（含 `ALLOW_INSECURE=1`）→ 正常模式、`/healthz` 200，行為與現行一致。
- `gitnexus_detect_changes`：唯一改動的 production 符號為 `main`（守門分支），`build_router` 未動，`build_misconfigured_router` 為純新增。

## Modified Files

- `docker-compose.yml`
- `.env.example`
- `docs/docker.md`
- `CHANGELOG.md`
- `crates/wukong-web/src/lib.rs`（新增降級 router＋測試）
- `crates/wukong-web/src/main.rs`（守門分支改降級常駐）
- `.claude/skills/wukong-release/SKILL.md`（breaking env 檢查）
- GitHub Release v0.17.0（線上，`gh release edit`）
- `docs/superpowers/plans/2026-07-08-web-allow-insecure-upgrade-fix.md`（checkbox 全數更新）

## 備註

- Task 2.2 的完整容器 runtime 測試（`docker compose up -d` + 跨機 curl）需重建 v0.17+ image；核心行為已由 compose config ＋ 真實 binary 端到端驗證涵蓋，重建 image 僅為再確認。
- 未提交（user 未要求 commit）。`git status` 中的 `CLAUDE.md`／`AGENTS.md`／`workspace/AGENTS.md` 為 gitnexus 索引自動更新，非本次改動。
