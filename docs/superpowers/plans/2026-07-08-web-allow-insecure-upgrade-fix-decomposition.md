# Implementation Plan Decomposition

> 來源計畫：`docs/superpowers/plans/2026-07-08-web-allow-insecure-upgrade-fix.md`（2026-07-08 Web Console 啟動守門升級破壞修復）
> 拆解原則：每個 Task 控制在 0.5～2 小時、可獨立完成與驗收；Phase 代表開發里程碑。原計畫 Task 3／4 超出單一 commit 粒度，已於此細切。
> 執行方式：以 `execute-task` 指定 Phase / Task 執行；每個 Task 完成即為一個 commit 邊界（1.2 為 `gh` 操作、2.2 為驗證，無 commit）。涉及 Rust 變更的 Task 完成後執行 `cargo test --workspace && cargo clippy --all-targets -- -D warnings`。

## 原計畫任務對照

| 原計畫 Task | Decomposition |
|---|---|
| Task 1（CHANGELOG + Release notes） | 1.1、1.2 |
| Task 2（`.env.example` + docker.md 止血） | 1.3、1.4 |
| Task 3（compose 治本） | 2.1、2.2、2.3 |
| Task 4（fail-visible 降級頁） | 3.1、3.2、3.3 |
| Task 5（release 流程檢查） | 4.1 |

---

## Phase 1 — 止血：升級資訊與範本補救

### Goal

讓已受影響的升級用戶不需讀原始碼、依 Release notes 一行解法即可恢復；讓新用戶照文件操作可一次啟動成功。本 Phase 完成即可先行發布（不需等 Phase 2）。

### Deliverables

* CHANGELOG v0.17.0 含「⚠️ 升級注意（Breaking）」區塊
* GitHub Release v0.17.0 notes 置頂同段升級注意
* `.env.example` 明示「token 或 allow-insecure 二選一」的啟動前置需求
* `docs/docker.md` 快速開始含前置步驟與診斷提示

### Dependencies

* 無（可立即開始；1.2 依賴 1.1 的定稿文案）

### Tasks

#### Task 1.1 CHANGELOG 升級注意區塊

* 任務說明：在 `CHANGELOG.md` 0.17.0 區塊頂部新增「### ⚠️ 升級注意（Breaking）」：現象（Docker 部署 `WUKONG_WEB_TOKEN` 為空時 `wukong-web` 拒絕啟動並不斷重啟、8787 無法連線）、二選一解法（`WUKONG_WEB_TOKEN=<secret>` 建議；`WUKONG_WEB_ALLOW_INSECURE=1` 僅限可信內網）、診斷指令（`docker compose logs wukong-web`）。
* 預期輸出：CHANGELOG 該區塊可被直接複製到 Release notes 使用。
* 涉及模組或檔案：Modify `CHANGELOG.md`
* 預估：0.5～1 小時

#### Task 1.2 GitHub Release v0.17.0 notes 更新

* 任務說明：`gh release view v0.17.0 --json body -q .body` 取現有 notes，將 Task 1.1 定稿的升級注意置頂合併，`gh release edit v0.17.0 --notes-file <file>` 更新。屬線上操作，無 commit。
* 預期輸出：GitHub Release v0.17.0 頁面第一屏可見升級注意與解法。
* 涉及模組或檔案：GitHub Release v0.17.0（`gh` CLI）
* 依賴：Task 1.1
* 預估：0.5 小時

#### Task 1.3 `.env.example` 補啟動前置需求

* 任務說明：改寫 Web Console 區塊：說明 Docker 部署必須二選一否則 `wukong-web` 拒絕啟動，列出 `WUKONG_WEB_TOKEN=<secret>`（建議）與 `WUKONG_WEB_ALLOW_INSECURE=1`（僅限可信內網）兩行範例。若與 Phase 2 同輪執行，可直接採 2.3 的新語境（預設僅 localhost 可達）一次寫到位，2.3 僅做核對。
* 預期輸出：新用戶複製 `.env.example` 後可理解且能一次啟動成功。
* 涉及模組或檔案：Modify `.env.example`（Web Console 區塊，現況 `:23-26`）
* 預估：0.5 小時

#### Task 1.4 `docs/docker.md` 快速開始補前置步驟

* 任務說明：快速開始在 `docker compose up -d` 前補「設定 Web Console 存取方式」一步，附 `docker compose logs wukong-web` 診斷提示；全文搜尋其他引用 8787 快速開始的文件（README 等）一併核對。環境變數表（`:151-153`）確認與行為敘述一致。
* 預期輸出：照 `docs/docker.md` 三步驟操作不再開箱即壞。
* 涉及模組或檔案：Modify `docs/docker.md`；必要時 `README.md`
* 預估：0.5～1 小時

---

## Phase 2 — 治本：安全邊界搬到 host port mapping

### Goal

compose 預設恢復開箱即用：host 端只綁 `127.0.0.1`（localhost 直接能用、外部進不來），容器內固定聽 8787 並明示 `WUKONG_WEB_ALLOW_INSECURE=1`；同時修掉 `WUKONG_WEB_PORT` 一變數兩用的埠不一致 bug。裸跑（非 Docker）fail-closed 行為不變。

### Deliverables

* `docker-compose.yml`：`ports: "${WUKONG_WEB_BIND:-127.0.0.1}:${WUKONG_WEB_PORT:-8787}:8787"`、容器內固定 8787、`WUKONG_WEB_ALLOW_INSECURE` 預設 `1` 附安全註解
* 三情境實測通過（預設 loopback／改埠／對外開放 + token）
* `.env.example`、`docs/docker.md` 與新預設一致（含 `WUKONG_WEB_BIND` 說明）

### Dependencies

* 無硬依賴 Phase 1（不同檔案段落）；但建議 Phase 1 先完成，避免同檔案措辭前後衝突（見 1.3 備註）

### Tasks

#### Task 2.1 `docker-compose.yml` 綁定與埠語意改寫

* 任務說明：(1) ports 改 `"${WUKONG_WEB_BIND:-127.0.0.1}:${WUKONG_WEB_PORT:-8787}:8787"`；(2) 移除 environment 中的 `WUKONG_WEB_PORT` 傳遞（容器內固定 8787，`WUKONG_WEB_PORT` 只作 host 端埠），healthcheck 目標同步固定容器內 8787；(3) `WUKONG_WEB_ALLOW_INSECURE` 改 `${WUKONG_WEB_ALLOW_INSECURE:-1}`，緊鄰註解說明「容器內必綁 0.0.0.0，實際暴露面由 `WUKONG_WEB_BIND` 控制；對外開放必須設 `WUKONG_WEB_TOKEN`」。`docker compose config` 驗證渲染結果。
* 預期輸出：`docker compose config` 呈現預期的 ports 與 environment；SECURITY 註解與新設計一致。
* 涉及模組或檔案：Modify `docker-compose.yml`（wukong-web 服務，現況 `:91-135`）
* 預估：1 小時

#### Task 2.2 三情境實測驗證

* 任務說明：實際 `docker compose up -d` 驗證：(a) 空 `.env`（模擬 v0.16.x 升級用戶）→ `curl http://127.0.0.1:8787/healthz` 通、容器不重啟、非 loopback 介面不可達；(b) `WUKONG_WEB_PORT=9000` → `curl http://127.0.0.1:9000/healthz` 通；(c) `WUKONG_WEB_BIND=0.0.0.0` + `WUKONG_WEB_TOKEN` → 帶 `Authorization: Bearer` 可用、無 token 請求被拒。純驗證，無 commit；發現問題回修 2.1。
* 預期輸出：三情境皆通過的驗證紀錄（附於 PR 或計畫文件 checkbox）。
* 涉及模組或檔案：驗證操作（`docker compose`、`curl`）
* 依賴：Task 2.1
* 預估：1～2 小時

#### Task 2.3 文件同步新預設語境

* 任務說明：`.env.example` 新增 `WUKONG_WEB_BIND` 說明、把 Web Console 區塊措辭改為新語境（預設僅 localhost 可達、無需設定即可用；對外開放的條件與作法）；`docs/docker.md` 環境變數表新增 `WUKONG_WEB_BIND`、更新 `WUKONG_WEB_PORT` 語意（host 端埠）與對外開放指引；快速開始若因 1.4 加了前置步驟、於新預設下已非必要，改為「預設即可用」並保留對外開放的指引。CHANGELOG 於下一版（如 v0.17.1）記載行為變更。
* 預期輸出：文件與新預設完全一致，無殘留舊語境。
* 涉及模組或檔案：Modify `.env.example`、`docs/docker.md`、`CHANGELOG.md`
* 依賴：Task 2.1、2.2（行為定案後再寫文件）
* 預估：0.5～1 小時

---

## Phase 3 — 體驗：fail-crash 改 fail-visible（可另開一輪）

### Goal

守門觸發時不再 `exit(1)` 進入 crash loop，改綁定原位址埠回 503 設定說明頁、`/healthz` 回 503 讓 healthcheck 如實標記 unhealthy。安全上仍 fail-closed：不掛任何功能路由。

### Deliverables

* `build_misconfigured_router()`：任意路徑 503 + 說明頁（含兩種 env 解法），`/healthz` 同樣 503
* `main.rs` 守門分支改走降級 router 常駐
* 誤配置情境下 `docker compose ps` 顯示 unhealthy 而非 Restarting

### Dependencies

* 無程式碼依賴；建議於 Phase 2 之後執行（新 compose 預設下此情境僅剩「使用者明示對外綁定但漏設 token」與裸跑誤配置）
* 執行時依 CLAUDE.md 規範先跑 `gitnexus_impact`（`main` 與 `should_refuse_insecure_start` 呼叫點）

### Tasks

#### Task 3.1 `build_misconfigured_router()` 與單元測試

* 任務說明：`lib.rs` 新增 `build_misconfigured_router()`：任意路徑回 503 + 靜態 HTML／純文字說明（`WUKONG_WEB_TOKEN` / `WUKONG_WEB_ALLOW_INSECURE=1` 兩種解法與 `docs/docker.md` 連結）；`/healthz` 同樣 503。測試：`/`、`/healthz`、任意 API 路徑皆 503 且 body 含指引文字；`should_refuse_insecure_start` 判斷邏輯與既有測試（`lib.rs:1213-1216`）不動。
* 預期輸出：新 router 與測試全綠；未觸碰 memory／backend／settings 相依。
* 涉及模組或檔案：Modify `crates/wukong-web/src/lib.rs`
* 預估：1～2 小時

#### Task 3.2 `main.rs` 守門分支改走降級 router

* 任務說明：守門觸發時保留既有 stderr 警告，改以 `build_misconfigured_router()` 綁定原位址埠常駐，不再 `std::process::exit(1)`（`main.rs:18-29`）。手動驗證：誤配置啟動後瀏覽器開 8787 可見說明頁、`docker compose ps` 顯示 unhealthy、容器不重啟；正確配置行為與現行完全相同。
* 預期輸出：fail-visible 行為端到端成立。
* 涉及模組或檔案：Modify `crates/wukong-web/src/main.rs`
* 依賴：Task 3.1
* 預估：1 小時

#### Task 3.3 行為變更文件化

* 任務說明：`docs/docker.md` 與 CHANGELOG 記載新行為：誤配置時服務拒絕功能但頁面可見原因，容器不再 crash loop；healthcheck 語意（unhealthy = 設定錯誤或服務異常）。
* 預期輸出：文件與新行為一致。
* 涉及模組或檔案：Modify `docs/docker.md`、`CHANGELOG.md`
* 依賴：Task 3.2
* 預估：0.5 小時

---

## Phase 4 — 流程：release 檢查防再犯

### Goal

把「會擋啟動的 env 變更須同步使用者接觸面」固化進 release 流程，杜絕同類事故。

### Deliverables

* `wukong-release` 檢查清單含 breaking env 四同步點檢查

### Dependencies

* 無（可隨任一輪帶上；案例連結指向本計畫文件）

### Tasks

#### Task 4.1 wukong-release 檢查清單擴充

* 任務說明：在 release 前置檢查清單加入：「若本版新增／變更會影響啟動或預設行為的環境變數：`.env.example`、`docker-compose.yml`、`docs/docker.md` 環境變數表必須同步，且 CHANGELOG 與 Release notes 需含『升級注意』區塊」；附 v0.17.0 `WUKONG_WEB_ALLOW_INSECURE` 事故連結（`docs/superpowers/plans/2026-07-08-web-allow-insecure-upgrade-fix.md`）作為案例。
* 預期輸出：後續 release 依 skill 流程會被迫核對 env 變更同步點。
* 涉及模組或檔案：Modify `.claude/skills/wukong-release/SKILL.md`
* 預估：0.5 小時

---

## 全域驗收與備註

1. 核心驗收：沿用 v0.16.x 舊 `.env`（無 token、無 allow-insecure）直接 `docker compose up -d --build`，`wukong-web` 正常啟動、`localhost:8787` 可用、容器不重啟；同時非 loopback 介面預設連不到 Web Console。
2. Phase 1 完成即可先行對外溝通（Release notes）；Phase 2 建議隨 patch release（如 v0.17.1）發布；Phase 3 可另開一輪；Phase 4 隨任一輪帶上。
3. 若 Phase 1 與 Phase 2 同輪執行：1.3／1.4 直接採 2.3 新語境一次寫到位，2.3 降為核對，避免同檔案改兩次。
4. Phase 3 涉及 Rust 符號變更，execute-task 時先跑 `gitnexus_impact`（CLAUDE.md 規範），commit 前跑 `gitnexus_detect_changes()`。
5. 完成各 Phase 後，回頭更新原計畫與本文件的對應狀態（checkbox）。
