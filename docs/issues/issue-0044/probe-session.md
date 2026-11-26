# Task 1.1 探測：V1 session 在 V2 遷移後可接續（SCN-001）

取樣：2026-11-24（UTC 07:3x～07:4x），主機 flashaim。只記筆數與 session 編號前綴，不記對話內容。

## 副本

正式部署 ~/Documents/RunWuKong 的三個容器（v0.25.1）照常執行、未重啟。以 #30 的 [snapshot.py](../issue-0030/scripts/snapshot.py) 唯讀取副本（volume 以 `:ro` 掛載、`--network none`，命令同 [#30 inventory 步驟 1](../issue-0030/inventory.md#副本取法與驗證)），副本放在本機 scratchpad，不進 repo：

| 資料庫 | 副本 byte | integrity_check | sha256 前綴 |
|---|---|---|---|
| memory.db | 1,925,120 | ok | 38904676 |
| opencode.db | 48,140,288 | ok | 9dc901ee |

## 執行

```bash
# SNAP 為副本目錄；V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK 為空目錄
bash docs/issues/issue-0044/scripts/probe-session.sh "$SNAP" "$V2" "$WORK"
```

[scripts/probe-session.sh](scripts/probe-session.sh)：V1（映像內 1.19.6）與 V2（2.3.2）各開一份 opencode.db 的再複製，全程 `--network none`，對話內容不離開本機。V1 的結果是比對 V2 用的獨立真相來源。

## 結果

memory.db 的 agent_session_state 有 4 個 scope 記錄了 session：

| session | V1 訊息數 | V1 最後一次壓縮後 | V2 `GET /api/session/{id}` | V2 `/context` 訊息數 | V2 訊息數 |
|---|---|---|---|---|---|
| ses_e165fe71… | 557 | 27 | 200 | 27 | 547 |
| ses_e15f2525… | 305 | 66 | 200 | 66 | 304 |
| ses_f7f9a674… | 2 | 2 | 200 | 2 | 2 |
| ses_f7f91dc0… | 201 | 24 | 200 | 24 | 202 |
| 負向對照 ses_doesnotexist… | — | — | 404 | 404 | — |

- V2 第一次啟動套用 24 個 migration（與 #30 步驟 4 相同）。
- 四個 session 的 V2 context 訊息數都等於 V1 從最後一次壓縮起算的訊息數，也就是 V2 接續時帶給模型的內容與 V1 相同。
- V2 的總訊息數與 V1 差幾則（547／557、202／201），是 V2 投影訊息的方式不同；接續看的是 context，這個差異不影響 SCN-001。
- 對 ses_e165fe71… 送 prompt：回 200，回應的 `sessionID` 等於原 session（沒有新建）；context 從 27 則變 29 則，原本的 27 則原樣保留在前面。

判定：SCN-001 成立。負向對照回 404，探測不會把查不到的 session 判為通過。

## 附帶觀察（交給後續 Task）

1. **沒有網路時回合不結束**：送出 prompt 後等 60 秒，session 仍在 `/api/session/active`。日誌只有一筆 `Failed to fetch models.dev`（models.opencode.ai 連不到），之後沒有任何 session 事件或錯誤。上游連不到時 V2 可能一直等，而不是回錯誤；這正是 SCN-007「不變成逾時」要防的情況，記為 README TBD-3，由 Task 1.3 取得真實錯誤樣本、Task 2.3 處理。
2. **檔案監看範圍**：V2 啟動時對 `/`、`/etc`、HOME 與 workspace 都建立 watcher。容器內 CPU 與 inotify 的成本未量，交給 Task 6.2 的版本對照一起看。
