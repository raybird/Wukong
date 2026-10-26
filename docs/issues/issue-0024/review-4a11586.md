# 審查報告

- 日期：2026-10-26
- 範圍：完整 PR #25 diff；本次交付判定涵蓋 SCN-001／SCN-002 程式修正與文件
- TARGET：`main`
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`
- Reviewed HEAD SHA：`4a115866fede1ac675f53754a68f2739be762e6a`
- Reviewed patch-id：`b9050ff7135d8ae7ef6ed563cb61ab8877276b2a`
- 獨立 reviewer：Codex 隔離 subagent `/root/review_4a11586`，非原實作者；主代理交叉核對並保存報告
- Review artifact：`docs/issues/issue-0024/review-4a11586.md`
- 2026-10-26 以 `gh pr view 25 --json baseRefOid,headRefOid` 確認本機與遠端固定範圍一致，收尾複查未變更。

## 問題與風險

- MUST FIX：無。
- SHOULD FIX：無新增項目。
- NICE TO HAVE：title／subagent 程序測試使用相似 mock fixture，未見行為分歧造成產品錯誤，屬可維護性議題，不阻塞本次修正。

### 前次發現覆核

`docs/docker.md:276` 已移除 Web 清空模型的錯誤指引，改為人工將 `/data/settings.json` 的 `agent.default_model` 設為 `null` 或刪除欄位。`crates/wukong-settings/src/lib.rs` 的 `AgentSettings.default_model: Option<String>` 支援這兩種反序列化寫法，`effective_agent_settings` 後續取得 `None`，與文案一致。

Web／REPL 不接受空值的描述符合實作；REPL `/set_models <model>` 為既有正確設定入口。前次錯誤指令、Web 清空操作與更早的 title／subagent 誤殺問題皆已修復。

## 已查核維度

| 驗收／失敗面 | 覆核結果與證據 |
|---|---|
| SCN-001：額度拒絕立即失敗並保留原因 | gateway 契約測試確認不待 timeout；分類及轉義引號解析保留原始原因。scheduler 測試確認 execution output 失敗標記與原因，不將該測試描述成正式 run 資料庫寫入驗證 |
| SCN-002：正常回覆與背景失敗 | title／subagent 錯誤後主要回合成功有程序與單元回歸測試；前輪 socket 測試確實執行 |
| 真正無回應、取消與啟動失敗 | 檢查既有 deadline、程序停止、process group 與 stdout／stderr drain 釋放；既有回歸證據維持有效 |
| 非 ERROR 日誌、原始訊息與分類 | 非 ERROR 忽略、轉義引號完整提取、額度字詞大小寫辨識有測試覆蓋 |
| 設定操作與移除覆寫 | Web 路由、REPL 解析／保存、空值拒絕與 Option 設定格式均與更新後文件相符 |
| 需求與核准 | SCN-001／SCN-002 驗收語意未變；SCN-003／TBD-1 維持待核准，本次無模型變更或正式部署操作 |

完整程式 diff、相關文件與歷史審查記錄均已覆核。未見新增 schema、依賴版本、權限或反向依賴；沒有其他可確定功能缺陷或阻塞性過度設計。

## 2026-10-26 證據重用與限制

`git diff --name-status 9f9b3a0 4a11586` 僅含 Docker 文件、implementation-plan 與歷史報告。`git rev-parse 9f9b3a0:crates HEAD:crates` 兩者均為 `6585feb0783258b110eb40d464f625d48a5e52c3`；程式、測試、執行設定與環境未變，因此重用既有程式驗證結果。本輪以靜態契約檢查驗證純文件修改，沒有新增產品測試或宣稱重新執行舊測試。

| 命令／證據 | 來源與結果 |
|---|---|
| `cargo test -p wukong-gateway -p wukong-scheduler --locked` | [review-9f9b3a0.md](review-9f9b3a0.md)：exit 0；gateway 137、scheduler 51、CLI 整合 1 passed |
| `cargo test -p wukong-gateway local_process:: -- --nocapture` | 同上：exit 0；9 passed，title／subagent／quota stderr 實際出現，socket 測試未跳過 |
| `cargo fmt --all -- --check` | 同上：exit 0；本輪無 Rust diff |
| `cargo test -p wukong-web model_settings_reject_empty_model --locked` | [review-249257d.md](review-249257d.md)：exit 0；1 passed，與文件所述 Web 空值拒絕一致 |

完整 workspace／clippy 本輪未重跑。managed_actual 共 6 項既有 ignored，不構成實機模型驗收。SCN-003／TBD-1 的模型處置與正式恢復證據仍待決策及核准；本次不宣稱正式模型服務恢復或全部合併 gate 已完成。

## 流程判定

PASS

適用於固定 HEAD 的 SCN-001／SCN-002 程式修正與文件範圍。獨立審查完成、報告已保存、此範圍無剩餘 MUST FIX。SCN-003 仍待核准，整個 issue 尚未結案。報告尚未提交；後續若只提交本報告，須依 `verify-artifact.py` 驗證單一報告提交例外，其餘變更重新確認審查有效性。
