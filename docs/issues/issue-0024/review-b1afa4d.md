# 審查報告

- 審查日期：2026-10-26。
- 範圍：完整 PR #25；TARGET=`main`。
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`。
- Reviewed HEAD SHA：`b1afa4dbf020aafdf12eac705b1d8aff1dd29c4f`。
- Reviewed patch-id：`e93a6c6151af735996c3d26d9a5446876ccd4b87`。
- 獨立 reviewer：Codex 隔離 subagent `/root/review_b1afa4d`，非原實作者；主代理交叉核對證據並保存報告。
- Review artifact：`docs/issues/issue-0024/review-b1afa4d.md`。
- 2026-10-26 透過 `gh pr view 25 --json headRefOid,baseRefOid` 確認遠端與本機範圍一致，審查收尾再次確認未變更。

## 問題與風險

### MUST FIX — P1：子代理可恢復的錯誤仍會中止主要回合

位置：`crates/wukong-gateway/src/local_process.rs:374–380`，呼叫處 `:330–331`。

新版僅排除 `agent=title`／`small=true`，但 OpenCode task 子代理也會產生 `small=false agent=general mode=subagent` 的串流錯誤。這些錯誤可能經 OpenCode 重試恢復，不應被視為主要 session 的致命失敗。現有 parser 仍將其送進錯誤通道，`execute` 隨即返回並停止整個 server，阻斷本可成功的主要回合。

2026-10-26 查核真實本機日誌 `/home/raybird/.local/share/opencode/log/opencode.log:54322`、`:54325`：2026-07-26 同一子代理 session 兩次 stream error 後，`:54327–54339` 顯示重試、工具操作與退出子代理 loop，`:54340–54346` 接回 `agent=build mode=primary`。證據僅取請求類型與狀態順序，不複製私人任務內容。

獨立 reviewer 從 HEAD 抽出 `parse_local_stream_error`、`is_auxiliary_stream_error`、`find_closing_quote`，於臨時 Rust probe 輸入相同形狀的 `mode=subagent` 日誌，得到 `Some(UpstreamFailed { kind: Other, ... })`。probe 用最小型別與分類 stub，來源篩選使用 HEAD 原函式；stub 不影響是否被判致命錯誤。

建議依本次主要 session／請求用途歸屬錯誤，補上「子代理先失敗後恢復、主要回合成功」測試。先前標題案例已修復，但尚未完整滿足僅由本次主要執行錯誤中止回合的目標。

### MUST FIX — P2：模型處置文件漏掉 Wukong 的明確模型設定

位置：`docs/docker.md:275`。

文件僅建議在 OpenCode `user.json` 配置替代模型。本事故 `/data/settings.json` 已設定 `agent.default_model="opencode/big-pickle"`；runtime `turn.rs:260`／`:326` 將其傳入 `AgentRequest.model`，gateway `opencode_server.rs:325`／`:341` 再傳入明確 `ModelOverride`。只調整 OpenCode 預設模型，最後一棒仍會被 Wukong 指定回已耗盡額度的模型。

建議補充經現有設定介面修改 Wukong `agent.default_model`，或清除明確覆寫後交由 OpenCode 選模型。這是文件修正，未授權本次操作正式部署。

## 已查核維度

- 驗收與規格：已讀 README 與 implementation-plan；SCN-001、SCN-002 已核准，SCN-003／TBD-1 仍待核准，未把正式模型可用性當成已恢復。
- SCN-001：額度拒絕契約測試確實立即返回原因；scheduler 測試確認 execution output 為失敗並含額度分類與原因。新增 scheduler 測試未直接查資料庫 run 狀態，不將它描述成資料庫寫入測試。
- SCN-002：title 背景失敗且主要成功的案例實際通過；子代理可恢復錯誤缺少回歸測試，見 P1。逾時 deadline 維持既有保護。
- 原始錯誤：轉義引號不截斷、非 ERROR 日誌忽略、額度關鍵字大小寫辨識均有測試。
- 程序生命週期：檢查取消、啟動失敗、回合失敗、stdout／stderr drain 與停止程序路徑；相關既有測試通過。
- 架構與權限：無 schema 變更、無反向依賴、無新增部署權限。正式部署未操作。
- 品質：檢查完整程式 diff 與跨任務重複，未見其他可確定缺陷或阻塞性的過度設計。
- GitNexus：compare 偵測 37 個變更符號／10 檔；execute 圖譜未列 caller，classify_text 列 1 個直接 caller、LOW。未以零 affected processes 推論零影響，另循原始碼查呼叫與測試。

## 2026-10-26 本輪驗證

被測版本為上述固定 HEAD；工作區無未提交程式差異。

| 命令 | 實際結果 |
|---|---|
| `cargo test -p wukong-gateway local_process:: -- --nocapture` | exit 0；8 passed；title stderr 與 quota stderr 均有輸出，未出現 sandbox skipping，兩項 socket 測試確實執行 |
| `cargo test -p wukong-gateway -p wukong-scheduler --locked` | exit 0；gateway 136 passed；scheduler 51 passed；cli_backend 1 passed；既有 managed_actual 共 6 ignored，需獨立 OpenCode／模型 fixture |
| `cargo fmt --all -- --check` | exit 0 |
| `git diff BASE HEAD --check` | exit 2；既有新增文件的 Markdown 硬換行尾端空白及檔尾空行；不列為功能缺陷 |

未重新執行完整 workspace／clippy，不將前次其他版本的紀錄冒充本輪結果。本輪沒有先前 socket 禁制問題；綠燈仍未覆蓋 P1 的子代理恢復路徑。

## 流程判定

RETURN TO execute-task

兩項 MUST FIX 尚待修正。此報告僅綁定 b1afa4d，不改寫先前被審查版本的歷史判定，也不代表整個 issue 已可結案。
