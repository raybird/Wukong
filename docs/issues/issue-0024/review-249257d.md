# 審查報告

- 日期：2026-10-26。
- 範圍：完整 PR #25，TARGET=`main`。
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`。
- Reviewed HEAD SHA：`249257d3ce743fab005fcb9253c2f04a4814cda8`
- Reviewed patch-id：`f477879a760b0c9aaca8469d4c0b90f68e8419b3`。
- 獨立 reviewer：Codex 隔離 subagent `/root/review_249257d`，非原實作者；主代理交叉核對並保存報告。
- Review artifact：`docs/issues/issue-0024/review-249257d.md`。
- 2026-10-26 以 `gh pr view 25 --json headRefOid,baseRefOid` 確認本機與遠端範圍一致，收尾複查未變更。

## 問題與風險

### MUST FIX — P2：Web 設定頁不支援清空模型以移除覆寫

位置：`docs/docker.md:276`。

新文案稱 Web `/settings` 可以「清空以移除覆寫」，但現有前端 `crates/wukong-web/static/components/wukong-settings.js:71–74` 會顯示「模型不可為空。」並直接返回；後端 `crates/wukong-web/src/lib.rs:399–401` 也拒絕空值並回 HTTP 400。依文件操作會保留原本的 `agent.default_model`，無法退回 OpenCode 預設模型。

最小修正是移除 Web 清空建議；若仍要說明移除覆寫，明確描述人工移除設定檔中的 `agent.default_model`，並確認是否另有環境覆寫。這是文件契約更正，無須新增清空功能或操作正式部署。

### 前次問題覆核

- 子代理可恢復錯誤誤殺：已修復，程式與前輪相同，仍有 mode=subagent 單元與程序回歸測試保障。
- 不存在的 `wukong models set <model>`：已修復，新文案改用 REPL `/set_models <model>`，與 `wukong-cli/src/command.rs` 的解析及保存行為一致。
- Wukong 明確模型覆寫：已正確解釋 `agent.default_model` 優先於 OpenCode 預設值；本輪僅上述清空操作不符現況。
- 無其他可確定缺陷或阻塞性的過度設計。

## 已查核維度

- 驗收與規格：SCN-001、SCN-002 語意及核准來源未變；SCN-003／TBD-1 仍待核准，未把正式排程模型可用性當成已恢復。
- SCN-001：既有 gateway 契約測試驗證額度錯誤立即失敗並保留原因，scheduler 測試驗證 execution output 失敗標記與原因；不將後者冒充正式 run 資料庫寫入驗證。
- SCN-002：正常回覆、title／subagent 錯誤不誤殺、真正無回應的逾時保護、取消與啟動失敗釋放均已查核。原始錯誤的轉義引號、日誌等級與分類回歸維持前輪程式內容。
- 架構、安全與權限：完整 PR 未新增 schema、依賴版本或部署權限；沿既有 gateway／runtime／scheduler 邊界，未操作正式部署。
- 品質：檢查完整程式 diff、文件及跨任務重複；相似 mock fixture 屬可維護性議題，未見邏輯分歧導致產品錯誤。
- 本次未提交新報告，不適用僅新增報告提交的沿用例外；目前判定直接綁定上述 HEAD。

## 2026-10-26 證據與重用條件

`git diff --name-status 9f9b3a0 249257d` 僅有 `docs/docker.md`、implementation-plan 與新增的前輪報告；產品程式、測試與執行設定無差異。兩版 `crates` tree 均為 `6585feb0783258b110eb40d464f625d48a5e52c3`，環境未變，因此重用 [前輪報告](review-9f9b3a0.md) 的程式測試證據，不把它描述成這次重跑。

| 驗證 | 結果與來源 |
|---|---|
| `cargo test -p wukong-web model_settings_reject_empty_model --locked` | 本輪獨立 reviewer 實跑，exit 0，1 passed；確認 API 空值拒絕契約 |
| `cargo test -p wukong-gateway -p wukong-scheduler --locked` | 重用 9f9b3a0 證據：exit 0；gateway 137、scheduler 51、CLI 整合 1 passed |
| `cargo test -p wukong-gateway local_process:: -- --nocapture` | 重用 9f9b3a0 證據：exit 0；9 passed，title／subagent／quota stderr 實際出現，socket 測試未跳過 |
| `cargo fmt --all -- --check` | 重用 9f9b3a0 證據：exit 0；本次無 Rust diff |

原有 managed_actual 共 6 ignored，不是實機模型驗收。本輪未重跑完整 workspace 或 clippy。文件檢查以 CLI 解析、REPL 保存、Web 路由與前後端空值處理為依據，未在正式環境執行設定變更。

## 流程判定

RETURN TO execute-task

程式修正與前次 CLI 指令問題已覆核；新文案仍有一項 P2 MUST FIX。報告已保存，尚未提交。SCN-003／TBD-1 保持待核准，整個 issue 尚未結案。
