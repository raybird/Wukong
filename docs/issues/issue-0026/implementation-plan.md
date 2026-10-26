# Issue 26 實作計畫

## 設計方案

1. **核心原則**：向後相容優先。未指定模型時，完全維持既有 Fallback 至預設模型的行為；已存在 SQLite 的排程設定不能因欄位擴充而解析失敗。
2. **後端資料流**：
   - OpenCode 支援 `opencode models`（列出 `provider/model` 清單）。後端在 `wukong-web` 新增 `GET /api/models`，解析輸出為按 Provider 分組的資料結構 `{ providers: { [provider]: string[] } }`，並加入適度快取避免重複耗時查詢。
   - `JobKind::Turn` 新增 `model: Option<String>`，加上 `#[serde(default)]` 確保舊 JSON 反序列化自動為 `None`。
   - `SchedulerStore` 儲存時保留 `model`；`SchedulerExecutor` 執行排程回合時，若該 Job 具備 `model` 則覆寫該回合傳入的 `cfg.default_model`，否則沿用全域預設。
3. **前端元件**：
   - 封裝共用或獨立的兩級連動下拉選單邏輯：載入 Provider 列表 → 選擇 Provider → 動態更新 Model 選項。
   - 提供「自訂模型（手動輸入）」選項，兼顧特殊或未即時列出的模型。

## 實作步驟

1. 📝 **SCN-001：探測 OpenCode 模型查詢契約並實作後端 API**
   - 相依：無。
   - 產出：探測容器內 `opencode models` 與 HTTP API 的返回結構與耗時，實作 `wukong-web` 的 `/api/models` 端點及單元測試。
   - 完成判準：後端單元測試通過，端點能正確回傳結構化的 Provider 分組模型資料，且在 Provider 為空或 OpenCode 不可用時優雅降級回傳空集合而不 crash。
2. 📝 **SCN-003、SCN-004：擴充排程模型欄位與向後相容執行邏輯**
   - 相依：步驟 1。
   - 產出：`JobKind::Turn` 加上 `model: Option<String>`；更新 `SchedulerStore`、`SchedulerExecutor`、`wukong-cli` 命令列介面。
   - 完成判準：單元測試驗證舊版未含 `model` 欄位的 JSON 可成功反序列化為 `None`；測試驗證有指定模型的排程執行時正確傳入該模型，未指定時 fallback 至 `cfg.default_model`。
3. 📝 **SCN-002、SCN-003：Web Console 兩級連動下拉選單實作**
   - 相依：步驟 1、步驟 2。
   - 產出：更新 `wukong-settings.js` 與 `wukong-schedules.js`，呼叫 `/api/models` 載入 Provider / Model 兩級下拉選單。
   - 完成判準：前端頁面正確載入已配置的 Provider；切換 Provider 即時連動更新 Model 下拉清單；建立/編輯排程可成功儲存與顯示所選模型；設定頁能成功儲存 Chat 預設模型。
4. 📝 **交付、文件與獨立審查**
   - 相依：步驟 1 ~ 3。
   - 產出：更新 `docs/docker.md`、`CHANGELOG.md`，全量測試通過，通過獨立代碼審查並建立 PR。
   - 完成判準：`cargo fmt`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 通過，review 報告判定 PASS。

## 使用方式對照

| 介面 | 變更前 | 變更後 |
|---|---|---|
| Web Settings | 單一輸入框手動輸入模型 | 兩級選單：先選 Provider 再選 Model（可切換手動輸入） |
| Web Schedules | 排程建立時無法指定模型 | 彈窗提供 Provider / Model 兩級選單，可為個別排程指定模型 |
| CLI `schedule add-turn` | 只支援 `--scope` 與 `--prompt` | 支援選擇性 `--model <model>` 旗標 |

## 測試策略

| 驗收 | 層級 | 判準 |
|---|---|---|
| SCN-001 | 單元測試／整合測試 | `/api/models` 正確解析模型字串並分組，快取與降級機制有效 |
| SCN-002 | 前端／API 測試 | Settings 呼叫 API 成功設定並持久化 Chat 模型 |
| SCN-003 | 排程單元／整合測試 | 指定模型的 Job 執行時傳遞給 agent 該特定模型識別碼 |
| SCN-004 | 回歸與反序列化測試 | 舊版 JSON 無 model 欄位反序列化成功且行為與既有完全一致 |

## 檢查清單

- [ ] SCN-001 後端模型清單端點與契約測試已完成。
- [ ] SCN-003 / SCN-004 排程模型欄位擴充與向後相容測試已完成。
- [ ] SCN-002 / SCN-003 前端兩級連動 UI 實作完成。
- [ ] 全量測試與靜態檢查通過，獨立審查 PASS。
