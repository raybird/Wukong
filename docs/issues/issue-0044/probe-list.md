# Task 2.1 探測：V2 session 列表分頁與認證

取樣：2026-11-24，主機 flashaim，@opencode/cli 2.3.2，容器用 wukong:v0.25.1 映像，全程 `--network none`，不送提示詞。

```bash
# SNAP 為 #30 snapshot.py 的副本目錄；V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK 為空目錄
bash docs/issues/issue-0044/scripts/probe-list.sh "$SNAP" "$V2" "$WORK"
```

## 結果

| 項目 | V2 2.3.2 實測 | 對 Wukong 的影響 |
|---|---|---|
| 不帶 limit | 預設最新 50 筆（openapi 說明；7 筆時全部回來） | 一定要照 cursor 走，只取一頁會漏掉最舊的（地雷 W-6） |
| `limit=3` 走 cursor（7 個 session） | 頁數 3、3、1、0；最後一頁有 1 筆時 `cursor.next` 仍不為空，再取一次才是空頁、`next` 為 null；沒有重複、沒有遺漏 | 結尾要同時認「空頁」與「沒有 next」 |
| 順序 | 預設與 `order=desc` 由新到舊，`order=asc` 由舊到新，跨頁維持 | Wukong 用預設順序 |
| `limit=0`、`limit=abc` | 400 `InvalidRequestError` | 不會送 |
| `cursor=bogus` | 400 `InvalidCursorError` | 照一般錯誤回報 |
| `limit=100000` | 200，沒有上限 | Wukong 每頁要 1000 筆 |
| `/api/info` 沒帶認證、密碼錯誤 | 都是 401 `{"_tag":"UnauthorizedError","message":"Authentication required"}` | 就緒檢查遇到 401 明說認證被拒（SCN-004） |
| 使用者名稱不是 opencode（另以 `alice`＋正確密碼實測） | 401 | basic auth 使用者名稱固定 opencode，`WUKONG_AGENT_SERVER_USERNAME` 不再有作用 |
| `/global/health`、V1 的 `/session` | 200 text/html（網頁） | 只看狀態碼的檢查會誤判健康 |
| 遷移後的正式資料副本 | session 表 9 列，V2 列出 9 筆（兩頁），`order=asc` 同一組；`archived=true` 為 0 | 遷移不會讓舊 session 從列表消失 |

使用者名稱與 `/global/health` 兩項是同日在另一個 `--network none` 容器以 curl 等價的 python 請求取得，不在腳本內。
