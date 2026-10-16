# 專案客製規範

本檔由專案維護，初始化只在不存在時建立，更新上游時保留。規劃與交付前讀取；未填內容從 repo 的設定、現有文件與命令取得，不為填表猜測。

## 專案邊界與驗證入口

- **模組邊界與相依方向**：根目錄 `AGENTS.md`「架構」。15 個 crate 的依賴是單向的，改動沿既有方向走；需要反向依賴時先說明理由。
- **命令**：根目錄 `AGENTS.md`「常用指令」。CI（`.github/workflows/ci.yml`）只跑三道：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`。推 main 前在本機跑完這三道。
- **腳本測試**：`scripts/test-*.sh` 不在 CI 裡，完整清單以 `scripts/release.sh` 的 `run_required_checks` 為準。動到 compose、`.env.example`、`Dockerfile*`、`scripts/docker-entrypoint.sh` 或 `scripts/install.sh` 時，交付前跑 `bash scripts/test-docker-runtime.sh`，其餘依改動對應的腳本跑。
- **正式部署**：`~/Documents/RunWuKong` 是實際執行的 compose 部署，不是 source repo。部署現況（映像版本、cgroup、日誌、資料庫）向那裡的容器查，不向本 repo 的 tag 或 compose 檔推論。

## 提交與交付

- **訊息**：`type(scope): 中文摘要`，scope 可省；issue 相關提交加 footer `Refs: #ID`。訊息只寫變更描述，不加任何 AI 署名或 trailer。
- **分支與 PR**：issue 走 `issue-<四位編號>-<英文描述>`（例：`issue-0009-memory-turn-key`），PR base 為 `main`，squash 合併，標題帶 `(#PR)`。文件、設定、規則檔類提交直接放 `main`。
- **遠端**：GitHub `raybird/Wukong`，SSH remote 加 `gh` CLI 認證，不需要 token。PR 合併後 GitHub 會自動刪除 head 分支；清理前先 `git fetch --prune origin` 確認遠端實況。
- **push 與發版**：push 等使用者開口；發版只在使用者明說要發布時用 `scripts/release.sh vX.Y.Z`（流程見 `wukong-release` skill），版本號來自 git tag。
- **平台 issue 查詢**（唯讀）：`gh issue list --state all --limit 100 --json number,title,state,closedAt,createdAt`；懸置掃描要靠 `state` 做平台／README 雙向比對，所以一律帶已關閉的 issue，不用 `--state open`。issue 文件放 `docs/issues/issue-<四位補零的 GitHub 編號>/`，掃描起點為全部展開。
- **合併判定**（唯讀）：`gh pr list --state all --limit 20 --json number,state,mergedAt,title` 的 `mergedAt`，對照 README 狀態欄記載的 PR 編號與日期；取得到 `mergedAt` 即為確定，不必退回 Git merge commit 推測。

## 常青文件更新責任

| 變更 | 更新的文件 |
|---|---|
| 新增、移除 crate，或改變相依方向、`run_turn` 資料流；新增會踩到的設計事實 | 根目錄 `AGENTS.md`「架構」「關鍵設計細節」 |
| compose、Dockerfile、entrypoint、opencode 設定分層、容器環境變數 | `docs/docker.md`；新增的 `WUKONG_*` 變數同步寫進 `.env.example` |
| 安裝、升級、installer | `docs/installation.md` |
| 記憶寫入、召回、合併、`wukong-memoryd` API | `docs/memory.md` |
| CLI 子命令與旗標 | `docs/cli-reference.md` |
| Telegram／Web Console／session 控制行為 | `docs/entrypoints.md`；Web Console 操作流程另見 `docs/web-console-sop-v1.md` |
| 發版流程、RC 演練 | `docs/release-rehearsal.md` |
| 功能總覽或快速開始受影響 | `README.md` |
| 使用者看得到的行為變更 | `CHANGELOG.md` 的 `[Unreleased]`，發版時整理成版本區塊 |
| 調查正式部署的事故，或從其他專案帶入事故比對 | 新增 `docs/YYYY-MM-DD-<主題>-handover.md`，一個事件一份。已寫下的結論不改寫，後續發現加帶日期的「追補」段落；其中的待決事項記入 GitHub issue，handover 連到該 issue |

## 本地 gate 與案例

### Gate

- **對正式部署的查核預設唯讀**：重啟服務、改設定、刪資料前先取得使用者同意。競爭、逾時、破壞性測試用臨時工作目錄與獨立 scope，不在正式排程的工作目錄上做。
- **規模判定**：跨多個 crate 但沿既有相依方向、沒有 schema 變更的改動判 Medium（issue-0003、issue-0009）；同時要驗四個進入點與部署時判 Large（issue-0007）。

### 地雷

編號固定，不再適用的條目標註失效日期與原因，保留原編號。

- **W-1 把描述檔當成產物**（規則見根目錄 `AGENTS.md`「驗證紀律」）
  - 症狀：結論看起來有憑有據，但跟實際部署或產物不符，而且沒有任何東西報錯。
  - 觸發：用本機 checkout 的 tag、base compose、註解或 fixture 回答「部署現在是什麼樣子」。
  - 怎麼查：向產物要答案——RunWuKong 的 `docker inspect`、cgroup，bundle 用 `tar -tf`，索引用 `EXPLAIN QUERY PLAN`。
  - 證據：`docs/2026-08-16-runtime-resource-handover.md`「追補：2026-08-17 中午的落地驗證與更正」（映像版本、記憶體上限、「沒有可用升級」三處皆錯）；`docs/2026-08-14-memory-recall-verification-handover.md`。
- **W-2 用與被診斷機制同源的訊號判斷它有沒有壞**（規則見根目錄 `AGENTS.md`「驗證紀律」）
  - 症狀：壞掉與正常時讀數一樣，例如中文查詢的 `confidence` 恆為 0.000。
  - 觸發：寫召回或計分相關的斷言、健康檢查。
  - 怎麼查：斷言 `source_signals` 這類由查詢路徑決定的結構性訊號，不只斷言命中數。
  - 證據：`docs/2026-08-14-memory-recall-verification-handover.md` 第 1 節。
- **W-3 環境變數只接到部分容器**
  - 症狀：使用者在 `.env` 設了、完全沒效果，也沒有錯誤訊息。
  - 觸發：新增或改動 `WUKONG_*` 變數。compose 沒用 `env_file`，每個服務的 `environment` 要各自列出，漏掉一個服務不會報錯。
  - 怎麼查：`bash scripts/test-docker-runtime.sh`（檢查 `.env.example` 每個變數都有接線）；CI 不跑它。
  - 證據：`f0c11a0`（`WUKONG_OPENCODE_CONN_GRACE_SECS`）、`ad8bf75`（schedulerd 缺 `WUKONG_TG_TOKEN`）、issue-0005（保留天數只傳進 schedulerd）。
- **W-4 多個服務同時遷移同一個記憶庫**
  - 症狀：升級後某個服務啟動失敗（`duplicate column name`），有重啟政策的服務會自己恢復，因此容易被忽略。
  - 觸發：在 `wukong-memory` 新增 schema 遷移。web、telegram、schedulerd 由 compose 同時拉起，都開 `/data/memory.db`。
  - 怎麼查：新增的遷移走 `add_column_if_missing`，並跑 `cargo test -p wukong-memory --test concurrent_open`。
  - 證據：`4bddb40`（RunWuKong 升 v0.21.6 時 schedulerd 崩過一次）。
- **W-5 `wukong` 全域旗標放在子命令前會跑一個真的回合**（事實見根目錄 `AGENTS.md`「關鍵設計細節」）
  - 症狀：沒有報錯，子命令被當成 prompt 送給 agent。
  - 觸發：寫腳本、entrypoint 或驗證步驟時帶 `--db` 等旗標。
  - 怎麼查：指定資料庫改用 `WUKONG_MEMORY_DB`；回歸測試 `a_global_flag_before_a_subcommand_turns_it_into_a_prompt`。
  - 證據：`docs/issues/issue-0003/implementation-plan.md` 步驟 8 的「額外發現」。
- **W-6 opencode `GET /session` 不帶 `limit` 只回最新 100 筆**（事實見根目錄 `AGENTS.md`「關鍵設計細節」）
  - 症狀：被截掉的正是最舊的 session；小資料量的測試永遠碰不到。
  - 觸發：任何列舉 opencode session 的程式。
  - 怎麼查：測試資料造超過 100 筆，確認舊的仍被列出。
  - 證據：`docs/issues/issue-0003/implementation-plan.md` 的探測表（150 個 session 不帶參數回 100）。
- **W-7 `exit 0` 不等於上游服務了這次請求**
  - 症狀：模型下架或上游錯誤時，opencode 吐降級文字並以 exit 0 收場，排程與健康指標全綠。
  - 觸發：新增 agent 呼叫路徑、改成功判定、換模型。
  - 怎麼查：`crates/wukong-gateway/src/stream.rs` 的上游錯誤分類測試（`real_end_of_life_payload_is_classified_as_model_eol`）。
  - 證據：`docs/2026-09-08-model-eol-silent-failure-handover.md`、`ce631e1`。
- **W-8 opencode 的權限詢問沒人回應會卡到逾時**（事實見根目錄 `AGENTS.md`「容器內 opencode 權限」與「無人值守權限處置」）
  - 症狀：排程回合停在等待，直到 1200 秒 agent timeout；容器本身沒有 crash。
  - 觸發：改 opencode 權限設定、切換 CLI／server backend、新增無人值守的執行路徑。`--dangerously-skip-permissions` 只對 CLI backend 有效。
  - 怎麼查：opencode log 搜 `permission=`，對照同一 stream 有沒有 reply。
  - 證據：`docs/2026-08-06-docker-runtime-handover.md`。
- **W-9 本機 clippy 綠不代表 CI 綠**
  - 症狀：推上去才被新 lint 擋下。
  - 觸發：CI 用 `dtolnay/rust-toolchain@stable`，會自動跟進新版；本機 toolchain 通常較舊。
  - 怎麼查：推 main 後看 CI 結果；本機三道全綠仍要等 CI。
  - 證據：`caad443`（`chunks_exact(4)` 改 `as_chunks::<4>()`）。
