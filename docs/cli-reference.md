# CLI 參考

> ← 回到 [主 README](../README.md)｜相關文件:[記憶模型與服務](memory.md)、[各進入點](entrypoints.md)

## CLI 參數

| 參數 | 說明 | 預設 |
| :--- | :--- | :--- |
| `[PROMPT]...` | 要問的內容（位置參數，以空白接回）；**留空則進入互動 REPL** | 選填 |
| `--scope <SCOPE>` | 記憶 scope（`global` / `project:X` / `agent:X` / `user:X`） | `project:<cwd 資料夾名>` |
| `--db <URL>` | 記憶資料庫位置 | `$HOME/.wukong/memory.db` |
| `--agent-cmd <CMD>` | agent 指令（空白分隔） | `opencode run` |
| `--no-stream` | 關閉活動渲染，純文字一次輸出 | off（預設串流） |

環境變數：`WUKONG_MEMORY_DB`、`WUKONG_AGENT_CMD`、`WUKONG_STREAM`（設 `0` 等同 `--no-stream`）、`WUKONG_MD_DIR`（設定後每次 remember 同步把記憶鏡像成 per-scope markdown）、`WUKONG_BIN`（注入「排程能力」提示詞時使用的 `wukong` 指令路徑，預設 `wukong`）。

## 使用範例

```bash
# 互動 REPL（無參數）：多輪對話、session 接續、記憶持續累積
wukong
#   悟空 › 你好
#   🐵 悟空·oracle
#   ...（/exit、/quit 或 Ctrl-D 離開；/scope <x> 切換 scope）

# 基本：問一句（預設驅動 `opencode run`）
wukong "幫我重構這個函式"

# 指定底層 agent 指令
wukong --agent-cmd "opencode run" "這段程式為什麼會 panic？"

# 覆寫記憶 scope（預設依工作目錄為 project:<資料夾名>）
wukong --scope "global" "記住：我偏好 4 空格縮排"

# 關閉活動渲染（純文字一次輸出，適合管線）
wukong --no-stream "這段程式做什麼？" > out.txt

# 記憶維護（手動子命令；刪資料的操作都支援 --dry-run 預覽）
wukong memory snapshot                       # 健康快照（總數/類型/年齡/覆蓋率/候選數）
wukong memory consolidate --scope project:X  # 用 opencode 把零碎 event 聚合成 Summary
wukong memory prune --dry-run                # 預覽將刪的低價值/已摘要記憶
wukong memory export --dir ./mem-md          # 依 DB 全量重建 markdown 鏡像

# 排程（Docker 模式會預設啟動 wukong-schedulerd，自動按 cron 執行）
wukong schedule add-turn \
  --name "daily project check" \
  --cron "0 9 * * 1-5" \
  --scope project:Wukong \
  --prompt "Review recent memories and suggest today's highest-impact task."

wukong schedule add-maintenance \
  --name "nightly consolidate" \
  --cron "0 2 * * *" \
  --scope project:Wukong \
  --task consolidate

wukong schedule list
wukong-schedulerd
```

> **活動渲染**：預設開啟，execute 以 `opencode run --format json` 即時呈現——文字到 stdout、工具活動（`▸ 使用工具 …`）到 stderr。`--no-stream` / `WUKONG_STREAM=0` 退回純文字。（opencode 目前不吐逐 token，故顆粒度為片段／步驟級而非逐字。）

每次執行會在 stderr 顯示這回合化身的角色，例如：

```
🐵 悟空·fixer
<agent 的回答…>
```

## 記憶維護子命令

| 子命令 | 說明 |
| :--- | :--- |
| `memory snapshot [--scope X]` | 印出健康快照：總數、依 scope/類型、年齡分佈、embedding 覆蓋率、consolidation/prune 候選數 |
| `memory consolidate [--scope X] [--dry-run]` | 把該 scope 的零碎 event/note 聚合成 `Summary`（經 opencode 摘要），來源標記為已摘要；`--dry-run` 只列批次 |
| `memory prune [--scope X] [--dry-run]` | 刪除「已被摘要」或「老舊+未取用+低重要度」的記憶；`Decision`/`Skill`/`Summary` 永不刪；`--dry-run` 只列清單 |
| `memory export [--dir D]` | 依 DB 全量重建 markdown 鏡像（DB 為唯一真相來源，markdown 單向衍生） |

## 排程子命令

`wukong schedule` 會把排程定義存在同一個 SQLite 記憶資料庫。Cron job 由 `wukong-schedulerd` daemon 自動觸發；Docker 模式預設會啟動 daemon，`trigger` 可在沒有 daemon 的情況下立即執行單一 job。

| 子命令 | 說明 |
| :--- | :--- |
| `schedule list` | 列出所有排程 job |
| `schedule add-turn --name N --cron C --scope S --prompt P` | 新增定時 Wukong turn，執行時沿用既有 planner 自動選 role/skill |
| `schedule add-maintenance --name N --cron C --task snapshot\|consolidate\|prune [--scope S]` | 新增定時記憶維護 job |
| `schedule rm --id ID` | 刪除排程 job |
| `schedule enable --id ID` / `schedule disable --id ID` | 啟用或停用排程 job |
| `schedule trigger --id ID` | 立即執行單一 job，並記錄 run history |
| `schedule runs [--id ID] [--limit N]` | 查看最近執行紀錄 |

排程語意：

- Cron 使用 5 欄格式：`minute hour day-of-month month day-of-week`。
- V1 以 UTC 評估 cron，避免容器與 host timezone 不一致。
- 多個 daemon 同時執行時會用 DB lease claim job，避免同一輪 due job 被重複執行。
- Scheduled turn 需要底層 OpenCode provider/auth 已設定；Docker 模式會使用共用的 `opencode-config` volume。

Docker 模式下 schedulerd 預設會隨 `docker compose up -d` 啟動，讓排程功能安裝後即可運作。若你不想執行排程 daemon，可手動停止該 service：`docker compose stop wukong-schedulerd`。

## 用自然語言建立排程（Telegram / 對話）

除了手動下 `schedule add-turn`，**助手本身也知道自己具備排程能力**：每回合執行的系統提示詞會常駐注入一段「排程能力」說明（含當前 scope 與實際指令），所以你可以在 REPL / Telegram / Web 直接用自然語言交辦：

> 「每天早上九點幫我做一次專案回顧」

助手會透過底層 opencode 的 shell 能力，自行執行對應的 `wukong schedule add-turn --scope <當前 scope> --cron "0 9 * * *" --prompt "…"`（cron 由它換算）。

- **前提**：底層 agent（opencode）需具備 shell 執行權限，且 `wukong` 在其 PATH 上。若 `wukong` 不在 PATH，設定 `WUKONG_BIN=/絕對/路徑/wukong`，注入的指令會改用該路徑。
- **結果回送 Telegram**：當排程是從 Telegram 建立的（scope 形如 `user:tg-<chat_id>`），`wukong-schedulerd` 觸發後會把該回合結果**主動推回原聊天室**——成功送渲染後的 HTML、失敗送一行簡短錯誤。daemon 需設定 `WUKONG_TG_TOKEN` 才能投遞；設 `WUKONG_SCHED_NOTIFY=0` 可全域關閉。
- **權限詢問處置**：排程回合是無人值守的，opencode 送出的權限詢問**預設一律拒絕**，並把處置結果附在該 run 的訊息（`[無人值守權限]` 區塊）與 log。沒有這層策略時，詢問不會有人回答，opencode 會一直等到 `WUKONG_AGENT_TIMEOUT_SECS` 才失敗。若部署明確信任 container 隔離，可設 `WUKONG_SCHED_PERMISSION=allow` 讓權限請求自動允許一次；一般 question（非權限）在任何設定下都仍會被拒絕。
- 投遞為 best-effort：推送失敗只記 log，不影響 job 本身的成功狀態（仍記於 `schedule runs`）。
- 共用的 Telegram 傳輸層（client + scope 解析）抽於 `wukong-tg-client` crate，由 bot 與排程 daemon 共用。

## 歸檔與剪枝安全機制

- **歸檔分群規則 (Consolidation)**：執行 `consolidate` 時，系統會將擁有相同 `session_id` 的 Event/Note 記憶強制分在同一個 Batch 以維持對話脈絡；無 Session 的零碎筆記則依 `batch_size` 順序切塊。
- **安全剪枝防護 (Prune Guard)**：`prune` 操作只會安全刪除「已被歸檔 (consolidated) 的記憶」，或者是「老舊、未被召回且重要性低於閥值（預設 $< 0.5$）的 Event/Note」。**`Decision`（決策）、`Skill`（技能）與 `Summary`（摘要）這三種類型的記憶在任何情況下皆受到保護，永不被 prune 刪除。**
