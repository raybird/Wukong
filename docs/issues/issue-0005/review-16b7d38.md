# Issue 5 獨立審查報告

- 審查日期：2026-10-03。
- 範圍：完整 [PR #6](https://github.com/raybird/Wukong/pull/6)，包含兩份 Compose、runtime 回歸檢查與 issue README，共四個檔案。
- Reviewed TARGET SHA：`198e09235244df7d7b36b208a0393e3af8f6c350`（main）。
- Reviewed BASE SHA：`198e09235244df7d7b36b208a0393e3af8f6c350`。
- Reviewed HEAD SHA：`16b7d38115ca218f11ecca34b7564b8d3232ea21`
- Reviewed patch-id：`c0e31596768df54cdbeab3fb2cfa2773f3dcfd13`（`git patch-id --stable`）。
- 核准基線：`7a88277632637b35a38a5f54976590d7ce6e27f3`；AC-1、AC-2 語意與 HEAD 相同，核准來源是 2026-10-03 使用者接受補齊設定與回歸檢查。
- 獨立 reviewer：Codex 隔離 subagent `/root/review_issue5`；未參與實作，不兼任原實作者。
- Review artifact：`docs/issues/issue-0005/review-16b7d38.md`。
- 風險：High；錯誤的環境變數傳遞可能讓手動 prune 忽略停用值，刪除 session。先查配置產物、核對停用值及負向檢查，未執行資料刪除。

## 問題與風險

- MUST FIX：無。
- SHOULD FIX：無。
- NICE TO HAVE：無。

## 已查核維度

### 驗收、核准與證據

已閱讀固定 `BASE..HEAD` 的完整 diff；HEAD 相較先前 `8ba93b7` 僅調整 README 的交付狀態及 Markdown 尾端空白，Compose 與測試未改。`gh pr view 6 --json baseRefOid,headRefOid` 於 2026-10-03 回傳上述 BASE／HEAD，與本機被審查版本一致。

| 驗收 | 固定版本規格與證據 | 獨立查核結果 |
|---|---|---|
| AC-1 | `16b7d38115ca218f11ecca34b7564b8d3232ea21:docs/issues/issue-0005/README.md:20`；同檔第 44–96 行驗證紀錄與可重建 Python 探針 | PASS：兩份 Compose 的 Web／Telegram／schedulerd 各自傳入指定字面值；8 次真實 Compose 展開均符合未設定→30、0→0、7→7、空字串→空字串。 |
| AC-2 | 同版本 README 第 22 行與第 44–96 行；`scripts/test-docker-runtime.sh:205` 的逐服務 YAML 檢查 | PASS：BASE 的兩份 Compose 配合最終 regression script 確實得到四處缺少設定的紅燈；最終版本完整腳本綠燈。六個服務各自移除設定、改成 `:-30` 均失敗，並另補六次寫死 `7` 的負向檢查，均被拒絕。 |

2026-10-03 reviewer 執行的證據如下；核對新 HEAD 後再執行完整 runtime script，結果仍相同：

```text
bash scripts/test-docker-runtime.sh
exit 0
ok: all 36 WUKONG_* variables offered in .env.example reach a container
docker runtime persistence checks passed

bash -n scripts/test-docker-runtime.sh
exit 0

git diff --check 198e09235244df7d7b36b208a0393e3af8f6c350 16b7d38115ca218f11ecca34b7564b8d3232ea21
exit 0，無輸出

python3 /tmp/issue5-independent-probe.py
exit 0
PASS: 8 Compose expansions and 12 negative checks
```

`/tmp/issue5-independent-probe.py` 由固定 HEAD README 的 Python code block 原文重建，沒有更改斷言；命令、完整程式與逐組預期值均可由該 README 取回。實測 Docker Compose v2.35.1。探針呼叫真正的 `docker compose --env-file /dev/null -f <檔案> config --format json`，期望值是核准規格的字面值，不從 Compose 配置重算答案。

獨立紅燈的重建方式：以 `git archive 198e09235244df7d7b36b208a0393e3af8f6c350` 展開至獨立暫存目錄，僅覆蓋 `scripts/test-docker-runtime.sh` 為被審查版本，於該目錄執行 `bash scripts/test-docker-runtime.sh`。2026-10-03 結果為 exit 1，兩份 Compose 各自的 Web／Telegram 共四處回報 `got None`，schedulerd 設定存在也無法掩蓋遺漏。

額外寫死值負向驗證：對兩份真實 Compose 分別以 PyYAML 解析，六個服務每次僅將該服務變數替換為 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS=7`，保存獨立暫存檔；以被審查腳本的 `PY_RETENTION` 原文執行 `python3 - <暫存檔>`。2026-10-03 六次皆 exit 1，stderr 指向被修改的服務。

### 相關失敗面

| 輸入／狀態 | 預期 | 覆蓋與判定 |
|---|---|---|
| 變數未設定 | 六個服務皆得到 30 | 真實 Compose 展開：PASS。 |
| 變數為 0 | 六個服務皆保留 0，不回退到 30 | 真實 Compose 展開：PASS。 |
| 變數為 7 | 六個服務皆得到 7 | 真實 Compose 展開：PASS。 |
| 變數為空字串 | 六個服務皆保留空字串，維持停用語意 | 真實 Compose 展開：PASS；另唯讀核對 `crates/wukong-runtime/src/session_retention.rs:118`，既有 `from_value` 無法解析時回傳 0。 |
| 任一服務移除變數 | 測試因該服務失敗 | 六次逐服務移除負向檢查：PASS。 |
| 任一服務改成 `:-30` | 測試失敗，避免空字串被替換為 30 | 六次逐服務錯誤預設負向檢查：PASS。 |
| 任一服務寫死天數 | 測試失敗，避免忽略使用者設定 | 六次逐服務寫死 7 負向檢查：PASS。 |
| schedulerd 設定存在、Web／Telegram 缺少 | 完整 runtime script 不得通過 | BASE 完整歸檔上的獨立紅燈：PASS，四處 `got None`。 |

### 需求、架構、安全與品質

- 需求：四行 Compose 設定與逐服務檢查直接對應核准範圍；沒有新增 prune 行為、清理範圍或資料遷移。README 的交付狀態清楚指出尚未合併，獨立審查仍須由 artifact 判定。
- 架構：改動只在部署環境傳遞與既有配置檢查；沒有 Rust crate、API 或依賴方向變更。兩份 Compose 保持同一設定契約。
- 安全與權限：`${VAR-30}` 保留明確停用值，無新增權限或秘密；YAML 使用 `safe_load`，沒有執行配置中的內容。未觸及刪除機制。服務環境 list 與 map 都能查核，缺少服務或該設定會失敗。
- 依賴失敗面：PyYAML 已由既有腳本第 297 行使用，沒有新增套件依賴；解析或匯入失敗會被 `set -e` 阻止，不會產生假綠燈。
- 品質與 code-simplify：完整 diff 無跨 Task 邏輯分歧或過度設計。只補四行設定，局部 YAML 迴圈替代全檔字串檢查；未建立不必要抽象，也未順便修改其他邏輯。
- 測試真偽：既有全檔檢查會漏掉服務，新的檢查與負向測試直接相交於此失敗模式；真實 Compose 展開獨立驗證設定產物，不靠註解或 fixture 推論。紅燈原因是缺少目標配置，不是環境或依賴故障。
- 驗證層級與持久力：本改動僅負責 Compose 傳值，逐服務配置與真實展開涵蓋同一可觀察責任，因此合併內／外迴圈紅燈合理。持久化腳本保護六個服務的字面契約；四組展開探針全文保存在 README，可重建，對本次驗收有效。

### 豁免、待確認與限制

- 沒有 gate 豁免，沒有影響交付的待確認項目。
- 未啟動容器、未執行 prune、未刪除真實 session；未修改 Rust，未執行 Cargo 測試。這些限制與 README／PR 描述一致，本次配置修正不需以資料刪除驗證。
- 本 PASS 僅綁定上述被審查 HEAD；協調者若新增唯一、非 merge、直接後繼的提交，且只新增本報告，可依 `docs/agents/review-evidence.md` 的 artifact 例外驗證後沿用。其他新提交或變更範圍需重新審查。

## 流程判定

**PASS**。2026-10-03 完整獨立審查已完成並保存本報告，AC-1／AC-2 與重要失敗面均有有效證據，無 MUST FIX。
