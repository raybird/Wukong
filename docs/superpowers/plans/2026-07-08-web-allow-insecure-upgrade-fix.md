# Web Console 啟動守門升級破壞修復（WUKONG_WEB_ALLOW_INSECURE Upgrade Fix）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修復 v0.17.0 引入的 Web Console fail-closed 啟動守門在 Docker 部署下造成的無限重啟問題：先以文件與範本止血（讓受影響用戶一行解法），再把安全邊界搬到正確位置（host port mapping）讓 compose 恢復開箱即用，最後把 fail-crash 改為 fail-visible 並補上 release 流程檢查，避免同類問題再發生。

**背景:** v0.17.0 的安全修復（project-health-remediation Task 4）讓 `wukong-web` 在「綁定非 loopback + `WUKONG_WEB_TOKEN` 為空 + 未設 `WUKONG_WEB_ALLOW_INSECURE=1`」時直接 `exit(1)`。但 `docker-compose.yml` 預設 `WUKONG_WEB_HOST=0.0.0.0`（容器內要讓 port mapping 通就必須如此），加上 `restart: unless-stopped`，形成「啟動 → 拒絕 → 退出 → 自動重啟」的無限迴圈，`localhost:8787` 永遠 connection refused。**升級用戶（沿用舊 `.env`）與全新安裝（`.env.example` 的 token 是註解掉的）皆會命中**，而 `.env.example`、CHANGELOG、GitHub Release notes 均未提示這個 breaking 變更。2026-07-07 實際發生升級用戶無法訪問 Web Console 的回報。

**Tech Stack:** Rust（axum）、Docker Compose、GitHub Releases（`gh` CLI）。

**執行順序:** Phase 0 為止血，應立即完成並隨 patch release 發布；Phase 1 為治本，建議與 Phase 0 同一輪完成；Phase 2 為體驗強化，可另開一輪；Phase 3 為流程防再犯，隨任一輪帶上即可。每個涉及 Rust 變更的任務完成後執行 `cargo test --workspace && cargo clippy --all-targets -- -D warnings`。

---

## 問題總覽

| # | 問題 | 嚴重度 | 位置 | 任務 |
|---|------|--------|------|------|
| 1 | fail-closed 守門 × compose 預設 `0.0.0.0` × `restart: unless-stopped` → 無限重啟，升級與全新安裝皆中 | 高 | `crates/wukong-web/src/main.rs:18-29`、`docker-compose.yml:110-127` | Task 3 |
| 2 | `.env.example` 缺 `WUKONG_WEB_ALLOW_INSECURE`，未說明「token 或 allow-insecure 二選一」的啟動前置需求 | 高 | `.env.example:23-26` | Task 2 |
| 3 | CHANGELOG v0.17.0 僅在 Security 段落帶過一句，無「升級注意（Breaking）」區塊；GitHub Release notes 同缺 | 高 | `CHANGELOG.md`（0.17.0 Security 段） | Task 1 |
| 4 | `docs/docker.md` 快速開始（`docker compose up -d` → 開 8787）未提前置需求，照做即壞 | 中 | `docs/docker.md:64-73` | Task 2 |
| 5 | 安全邊界位置錯誤：容器內綁 `0.0.0.0` 是 Docker 必然，實際暴露面由 host 端 port mapping 控制，守門在 compose 情境下屬誤傷 | 中 | `docker-compose.yml:97-98` | Task 3 |
| 6 | `WUKONG_WEB_PORT` 覆寫時 port 對不上：容器內 app 改聽新 port，但 mapping 目標仍是容器的 8787 | 中 | `docker-compose.yml:98,114` | Task 3 |
| 7 | 拒絕啟動 = crash loop，使用者只見 connection refused，需翻 `docker compose logs` 才知原因 | 中 | `crates/wukong-web/src/main.rs:28` + `restart: unless-stopped` | Task 5 |
| 8 | release 流程無「會擋啟動的 env 變更須同步範本／文件／升級注意」檢查 | 低 | `.claude/skills/wukong-release/SKILL.md` | Task 6 |

---

# Phase 0（P0）：止血 — 文件與範本補救

彼此獨立，可平行。完成後受影響用戶依 Release notes 一行解法即可恢復。

## Task 1: CHANGELOG 升級注意 + GitHub Release notes 補充

**問題:** v0.17.0 的 CHANGELOG 只在 Security 段落寫「不安全綁定啟動即拒絕（fail-closed）」，沒有以升級者視角說明「你會遇到什麼、該做什麼」。GitHub Release v0.17.0 的 notes 同樣缺失，這是升級用戶最主要的資訊入口。

**Files:**
- Modify: `CHANGELOG.md`（0.17.0 區塊頂部）
- GitHub Release v0.17.0（`gh release edit`）

**Steps:**

- [x] **Step 1:** 在 CHANGELOG 0.17.0 區塊頂部新增「### ⚠️ 升級注意（Breaking）」小節，內容：
  - 現象：Docker 部署（或任何非 loopback 綁定）在 `WUKONG_WEB_TOKEN` 為空時 `wukong-web` 會拒絕啟動並不斷重啟，`localhost:8787` 無法連線。
  - 解法（二選一）：`.env` 加 `WUKONG_WEB_TOKEN=<secret>`（建議），或 `WUKONG_WEB_ALLOW_INSECURE=1`（僅限可信內網），然後 `docker compose up -d`。
  - 診斷指令：`docker compose logs wukong-web` 可見拒絕原因。
- [x] **Step 2:** `gh release view v0.17.0 --json body` 取現有 notes，把同一段升級注意置頂後 `gh release edit v0.17.0 --notes-file <file>` 更新。
- [x] **Step 3:** 若本計畫 Phase 1 隨 patch release（如 v0.17.1）發布，於該版 CHANGELOG 記載「compose 預設恢復開箱即用」的行為變更。

**驗收:** CHANGELOG 與 GitHub Release v0.17.0 皆含升級注意與可直接複製的解法；受影響用戶不需讀原始碼即可自救。

---

## Task 2: `.env.example` 與 `docs/docker.md` 補前置需求

**問題:** `.env.example` 的 Web Console 區塊只有註解掉的 `WUKONG_WEB_TOKEN`，完全沒提 `WUKONG_WEB_ALLOW_INSECURE`，也沒說不設會拒絕啟動。`docs/docker.md` 快速開始三步驟（up → 開 8787）現在照做即壞。

**Files:**
- Modify: `.env.example`（Web Console 區塊）
- Modify: `docs/docker.md`（快速開始一節；環境變數表已有兩變數說明，確認與新行為一致即可）

**Steps:**

- [x] **Step 1:** `.env.example` Web Console 區塊改寫：說明「Docker 部署必須二選一，否則 `wukong-web` 拒絕啟動」，列出 `WUKONG_WEB_TOKEN=<secret>`（建議）與 `WUKONG_WEB_ALLOW_INSECURE=1`（僅限可信內網）兩行範例。Phase 1 完成後再同步改為「預設僅 localhost 可達，無需設定」的新語境（見 Task 3 Step 4）。
- [x] **Step 2:** `docs/docker.md` 快速開始在 `docker compose up -d` 前補一步「設定 Web Console 存取方式」，並附 `docker compose logs wukong-web` 診斷提示。
- [x] **Step 3:** 全文搜尋其他引用 8787 快速開始的文件（README 等）一併核對。

**驗收:** 新用戶照 `.env.example` + `docs/docker.md` 操作可一次啟動成功；文件明確寫出不設定時的失敗模式。

---

# Phase 1（P1）：治本 — 安全邊界搬到 host port mapping

## Task 3: compose 預設 loopback 綁定 + 容器內固定埠

**問題:** 容器內綁 `0.0.0.0` 是 port mapping 的必要條件，fail-closed 守門在 compose 情境永遠觸發（問題 5）。真正該收斂的是 host 端暴露面：預設 `"8787:8787"` 綁 host 全介面。同時 `WUKONG_WEB_PORT` 一變數兩用（host mapping 與容器內綁埠）造成覆寫時 port 對不上（問題 6）。

**設計:** 安全邊界 = host port mapping。預設 host 端只綁 `127.0.0.1`（localhost 直接能用、外部進不來，比「全介面暴露靠 token 擋」更安全）；容器內固定聽 8787 並由 compose 明示 `WUKONG_WEB_ALLOW_INSECURE=1`（附註解說明安全性由 host 綁定保證）。要對外開放者明確設 `WUKONG_WEB_BIND=0.0.0.0` 並依文件要求設 token。裸跑（非 Docker）的 fail-closed 行為完全不變。

**Files:**
- Modify: `docker-compose.yml`（wukong-web 服務）
- Modify: `.env.example`、`docs/docker.md`（同步新變數與新預設）

**Steps:**

- [x] **Step 1:** ports 改為 `"${WUKONG_WEB_BIND:-127.0.0.1}:${WUKONG_WEB_PORT:-8787}:8787"`；移除 environment 中的 `WUKONG_WEB_PORT` 傳遞（容器內固定 8787，`WUKONG_WEB_PORT` 只作 host 端埠，順手修掉問題 6），healthcheck 目標同步固定為容器內 8787。
- [x] **Step 2:** environment 的 `WUKONG_WEB_ALLOW_INSECURE` 改為 `${WUKONG_WEB_ALLOW_INSECURE:-1}`，緊鄰註解說明：容器內必綁 0.0.0.0、實際暴露面由 host mapping（`WUKONG_WEB_BIND`）控制；對外開放時必須設 `WUKONG_WEB_TOKEN`。
- [x] **Step 3:** `docker compose config` 驗證渲染結果；實測三情境：(a) 空 `.env` → `curl http://127.0.0.1:8787/healthz` 通、他機不可達；(b) `WUKONG_WEB_PORT=9000` → `curl http://127.0.0.1:9000/healthz` 通；(c) `WUKONG_WEB_BIND=0.0.0.0` + token → 帶 `Authorization: Bearer` 可用。
- [x] **Step 4:** `.env.example` 新增 `WUKONG_WEB_BIND` 說明並把 Task 2 Step 1 的措辭改為新語境（預設僅 localhost 可達；對外開放的條件與作法）；`docs/docker.md` 環境變數表新增 `WUKONG_WEB_BIND`、更新 `WUKONG_WEB_PORT` 語意（host 端埠）與對外開放指引。

**驗收:** 舊 `.env`（無 token、無 allow-insecure）直接 `docker compose up -d` 後 `localhost:8787` 可用且不重啟；預設情況外部介面連不到 8787；`WUKONG_WEB_PORT` 覆寫後 host 埠與服務一致。

---

# Phase 2（P2）：體驗 — fail-crash 改 fail-visible（可另開一輪）

## Task 4: 拒絕啟動時改為綁埠回 503 設定說明頁

**問題:** 守門觸發時 `exit(1)`，在 `restart: unless-stopped` 下成為 crash loop；使用者在瀏覽器只看到 connection refused，必須翻容器 log 才知道原因（問題 7）。

**設計:** 守門觸發時不退出，改綁定同一位址埠、以極簡 router 對所有路由回 503 靜態頁（說明兩個 env 解法，等同現有 stderr 訊息的網頁版），`/healthz` 回 503 讓 compose healthcheck 如實標記 unhealthy。安全上仍是 fail-closed：不掛任何功能路由、不觸碰 memory／backend／settings。

**Files:**
- Modify: `crates/wukong-web/src/main.rs`（守門分支改走降級 router）
- Modify: `crates/wukong-web/src/lib.rs`（新增 `build_misconfigured_router()` 與測試；`should_refuse_insecure_start` 判斷邏輯不動）

**Steps:**

- [x] **Step 1:** `lib.rs` 新增 `build_misconfigured_router()`：任意路徑回 `503` + 靜態 HTML/純文字說明（含 `WUKONG_WEB_TOKEN` / `WUKONG_WEB_ALLOW_INSECURE=1` 兩種解法與文件連結）；`/healthz` 同樣 503。
- [x] **Step 2:** `main.rs` 守門分支改為：印出既有 stderr 警告後，改用降級 router 綁定原位址埠常駐（不再 `exit(1)`）。
- [x] **Step 3:** 測試：降級 router 對 `/`、`/healthz`、任意 API 路徑皆 503 且 body 含指引文字；既有 `should_refuse_insecure_start` 測試不變。
- [x] **Step 4:** `docs/docker.md` 與 CHANGELOG 記載新行為（拒絕服務但頁面可見原因，容器不再 crash loop）。

**驗收:** 誤配置時瀏覽器開 8787 直接看到原因與解法；`docker compose ps` 顯示 unhealthy 而非反覆 Restarting；正確配置後行為與現行完全相同。

---

# Phase 3（P3）：流程 — release 檢查防再犯

## Task 5: wukong-release 檢查清單納入 breaking env 檢查

**問題:** 本次事故的根因之一是「會擋啟動的 env 變更」沒有隨 release 同步到使用者接觸面（範本、compose、文件、升級注意），流程上無人把關（問題 8）。

**Files:**
- Modify: `.claude/skills/wukong-release/SKILL.md`

**Steps:**

- [x] **Step 1:** 在 release 前置檢查清單加入一條：「若本版新增／變更會影響啟動或預設行為的環境變數：`.env.example`、`docker-compose.yml`、`docs/docker.md` 環境變數表必須同步，且 CHANGELOG 與 Release notes 需含『升級注意』區塊」。
- [x] **Step 2:** 附本次事故（v0.17.0 `WUKONG_WEB_ALLOW_INSECURE`）作為案例連結到本計畫文件。

**驗收:** 之後任何 release 依 skill 走流程時會被迫核對 env 變更的四個同步點。

---

## 低優先觀察項（backlog，不排任務）

- **`restart: unless-stopped` 對配置錯誤的重啟語意**：Phase 2 完成後 crash loop 消失，此項自然解決；若 Phase 2 延後，可評估 `restart: on-failure:5` 讓錯誤停下來可見，但會犧牲暫時性錯誤的自癒能力，暫不動。
- **`wukong-memoryd` 同型守門**：目前預設 `WUKONG_MEMORY_HOST=127.0.0.1`，無此問題；若未來預設值改動需套用相同的邊界設計。

---

## 全域驗收標準

1. **升級路徑**：沿用 v0.16.x 舊 `.env`（無 token、無 allow-insecure）直接 `docker compose up -d --build`，`wukong-web` 正常啟動、`localhost:8787` 可用、容器不重啟。
2. **預設安全**：不改任何設定時，非 loopback 介面連不到 Web Console；裸跑非 loopback 綁定的 fail-closed 行為不變（`should_refuse_insecure_start` 測試全綠）。
3. **資訊入口**：CHANGELOG 與 GitHub Release v0.17.0 皆含升級注意與一行解法；`.env.example`、`docs/docker.md` 與實際行為一致。
4. 涉及 Rust 變更的任務：`cargo test --workspace && cargo clippy --all-targets -- -D warnings` 全綠。
5. 每完成一個 Phase，回頭更新本文件 checkbox。
