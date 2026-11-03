# 審查報告
- 範圍：完整 PR（`915dcd2..ab44d7c`）。重點是 ab44d7c 這個修正 commit，我也重新核對了它與前一次審查範圍 `915dcd2..e63beeb` 的關係。
- Reviewed BASE SHA：915dcd29ac14aa67dcece7d0b6090691ac343e22
- Reviewed HEAD SHA：ab44d7c5f95f837710b46fbb490509bc1b8b1c9d
- Reviewed patch-id：ec605e2dab789b5a2a1745a2cb8a87b0e16d4445（2026-11-03 以 `git diff 915dcd2 ab44d7c | git patch-id --stable` 重算，相符；`gh pr view 29` 的 headRefOid 也是 ab44d7c）
- 獨立 reviewer：Claude subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0028/review-ab44d7c.md
- 審查日期：2026-11-03
- 前一次審查：`review-e63beeb.md`（RETURN），本報告不修改它。ab44d7c 收進 repo 的那份與我當時寫的內容相同：收進後工作樹沒有差異。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX

1. **PR #29 的說明還停在 e63beeb，TBD-6 卻寫成「已在 PR 揭露」**（`docs/issues/issue-0028/README.md` TBD-6 那一列）。
   - 現況：2026-11-03 執行 `gh pr view 29`，PR 說明的交付範圍、HEAD 與 patch-id 還是 `e63beeb`／`e8262140…`。驗收表沒有 SCN-011。「限制與待確認」只有 TBD-1、4、5，沒有 TBD-6 第 1 項的內容：本機模式舊部署前幾輪會用滿 5 分鐘、這段期間排程掃描暫停、逾時那一輪沒有摘要行。
   - 影響：TBD-6 第 1 項之所以能「不處理」，前提就是已經揭露，現在這個前提不成立。依 `docs/agents/review-evidence.md`，PR 證據要對齊被審查的 BASE..HEAD。
   - 建議：把 PR 說明的範圍改成 `915dcd2..ab44d7c`（patch-id `ec605e2d…`），補上 SCN-011 一列（證據指向計畫步驟 6），「限制與待確認」補上 TBD-6 三項。這只是文件與平台內容，不必重審程式。

### NICE TO HAVE

1. **在本機 backend、沒有設定天數的情況下，`--dry-run` 的輸出沒有提醒「實際刪除會被拒絕」**（`crates/wukong-cli/src/main.rs:95-106`）。預覽仍以預設 30 天計算，會把主機上使用者自己的 session 列為「將刪除」。使用者已決定允許 dry-run，這不影響 SCN-011 成立。上次建議的一行提示（例如「本機模式沒有設定 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS`，實際刪除會被拒絕、schedulerd 也不會清」）沒有採用，可以考慮補上。
2. **同一條規則現在有兩份實作**：CLI 的 `local_prune_refusal` 與 schedulerd 的 `active_policy` 都各自判斷「本機 backend 加上沒有明確設定」。目前兩邊一致，也各有測試，但以後改一邊時容易漏掉另一邊。可以考慮把判斷收進 `wukong_runtime::session_retention`，兩個入口共用。不強求。

## 已查核維度

- **驗收與證據**：
  - 上次的 MUST FIX 已關閉。`local_prune_refusal` 在 `opencode prune` 分支一開始就判斷（`main.rs:95-98`），位置在 `RetentionPolicy::from_env()` 與 `prune_opencode_sessions` 之前，拒絕時 `exit(1)`。我追了這個分支裡被拒絕前會執行的程式：只有 `GatewayConfig::resolve`、開啟記憶庫（`memory.db`，不是 `opencode.db`），以及 `build_backend_from_env`（只建構 struct，不啟動行程）。被拒絕的那次執行不會啟動 `opencode serve`，也就碰不到 `opencode.db`。實際刪除只能經過 `prune_opencode_sessions`，全 repo 只有兩個非測試呼叫端：CLI 的這個分支與 schedulerd 的 `run_within`（後者有 `active_policy` 擋著）。`MaintenanceTask::Prune` 是記憶的 prune，與 opencode 無關。Web、Telegram、memoryd 都沒有入口。
  - 環境變數是空字串時：`retention_days_configured()` 用 `var_os`，回 `Some("")`，視為已設定，所以不會被拒絕。但 `from_value("")` 是 0 天，`prune_opencode_sessions` 在 `!policy.enabled()` 就直接返回，不列表也不刪除，`render_report` 印出「已停用」。值寫成無法解析的 `off`、`-1` 也一樣。方向是安全的。
  - 純 CLI backend 不會被拒絕，但 `list_sessions` 走 trait 預設的錯誤，照樣安全失敗，與 `docs/cli-reference.md` 寫的「純 CLI backend 不支援」一致。server backend 的行為不變（SCN-003）。
  - SCN-011 的證據成立：
    - 單元測試 `local_prune_without_configured_days_only_previews` 涵蓋四種情況：本機未設定且實際刪除時拒絕、本機未設定加 dry-run 時放行、本機已設定時放行、server 時放行。紅燈是空實作；兩個變異（拿掉 `!dry_run`、拿掉 `!configured`）都紅在對應的斷言上，所以這支測試在錯誤行為下不會是綠的。
    - 單元測試沒有涵蓋「main 有沒有接上這個判斷」。這一點由三次真實 binary 執行補上（opencode 1.19.4 沙盒）：未設定時實際刪除 rc=1、session 數不變；`--dry-run` rc=0、session 數不變；設定 30 天後實際刪除 1 個，只剩被指向的 session；三次都沒有殘留行程。這對應到 Gherkin 的每一個 Then，包括「opencode.db 的 session 一個都沒有被刪除」。
    - 核准來源記在 README（使用者選「允許 dry-run（建議）」），核准表也有 SCN-011。
    - 我在 ab44d7c 重跑：`cargo test -p wukong-cli --bin wukong local_prune`（1 passed）。
  - SCN-007 的補強成立：新增「沒有 `/compact`」與「`session_ids[1] == None`」兩個斷言（index 1 是最後一棒，index 0 是 planner）。把 `ROTATE_AFTER_MS` 改成 `i64::MAX` 的變異紅在 `turn.rs:1421`，現在可以直接證明這次失敗的是輪替回合。我重跑 `a_rotation_turn_that_fails_keeps_the_old_session`，1 passed。
  - 全套結果依計畫步驟 6 的紀錄：fmt、clippy、workspace 718 支全過。我只重跑了上面兩支，其餘沒有重跑。

- **相關失敗面**：

  | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
  |---|---|---|---|
  | 本機、未設定、實際刪除 | 拒絕、rc=1、不啟動 opencode | 單元測試，加真實 binary 第 1 次 | 正確 |
  | 本機、未設定、`--dry-run` | 列出、不刪 | 單元測試，加真實 binary 第 2 次 | 正確；輸出可能誤導，見 NICE TO HAVE 1 |
  | 本機、空字串或無法解析的值 | 不拒絕，但 policy 停用，不列也不刪 | 讀碼（`from_value`、`prune_opencode_sessions` 開頭的 `!enabled` 檢查） | 安全 |
  | 本機、明確設定 | 照常清理 | 單元測試，加真實 binary 第 3 次 | 正確 |
  | server／純 CLI backend | 行為不變；CLI 在列表時失敗 | 單元測試（server），加讀碼 | 正確 |
  | 其他 prune 入口 | 都有閘門 | 用 grep 確認只有 CLI 與 schedulerd 兩個呼叫端 | 已全部涵蓋 |

- **需求、架構、安全、品質**：
  - 新 commit 只改了 `wukong-cli` 一個純函式與它的呼叫點、一支測試的斷言，以及文件。沒有改 gateway、runtime、schedulerd 的行為，前一次審查的其餘結論仍然有效。
  - 我沒有發現 ab44d7c 帶進新的正確性問題。重複實作見 NICE TO HAVE 2。
  - 文件：`docs/cli-reference.md:93` 已改成「支援 server 與本機；本機沒有設定時只允許 dry-run；純 CLI 不支援」，`docs/docker.md` 與 `CHANGELOG.md` 補了同一個條件，三者都與程式碼相符。上次的 SHOULD FIX 已關閉。

- **豁免、待確認與限制**：
  - 我認為 TBD-6 合理：
    - 第 2 項只是日誌雜訊，不影響行為。
    - 第 4 項我上次已確認，server 與本機 backend 一定會回 session id，純 CLI 不依年齡輪替，所以實際走不到。不處理可以接受。
    - 第 1 項是既有結構，影響有限：只發生在舊部署的前幾輪，排程最多延後 5 分鐘，不會遺失資料。它能不處理的前提是 PR 真的有揭露，目前還沒有，見 SHOULD FIX 1。
  - TBD-1（7 天是暫定值）也還沒寫進新版的 PR 說明。現在的說明裡有，但範圍仍是舊的，更新 PR 說明時要保留。
  - 真實 binary 的三次執行，我是依 implementation-plan 步驟 6 的紀錄判讀，沒有自己重跑。

## 流程判定
PASS
