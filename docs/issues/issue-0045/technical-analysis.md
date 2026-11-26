# 技術分析：方案取捨

> ← 回到 [README](README.md)｜現況見 [requirement-analysis.md](requirement-analysis.md)

2026-11-26 使用者逐題確認後的取捨。V2 的能力（agent、subagent、skill、session instruction entries）在 2026-11-24 已有初步探測，結果記在 GitHub #45 內文；尚未確認的部分是 Phase 1 的首要驗證。

## 1. 規劃改由模型決定

```diff
 run_turn
-  plan（另一次 LLM 呼叫，plan agent 輸出「角色|技能」，最多 3 行）
-  for 每一棒
-    輔助棒：無 session、帶上一回合補償
-    最後一棒：scope 的 session
+  scope 的 session，以 primary agent「wukong」送出一次 prompt
+    模型自己決定直接回答，或用 task 工具派工給 explorer／oracle／librarian／fixer／designer
+    入口顯示的角色從 V2 事件裡的 subagent 資訊取得
```

- **好處**：少一次 LLM 呼叫，本機模式也少啟動一次 opencode。subagent 由 primary agent 交代任務，不必再補上一回合。
- **代價**：路由從程式碼決定變成由模型決定，unit test 證明不了，要用真實模型量測。
- **以哪個模型判定**：原本依 2026-11-26 使用者的 Q1 決定以 NVIDIA nemotron 為準。同日 NVIDIA 憑證被拒（403）後，使用者表示「使用免費模型驗證也可以」：NVIDIA nemotron 或預設免費模型 opencode/big-pickle 都可以判定，紀錄寫明實際用的模型，對照組用同一個模型。

## 2. agent 定義由 Wukong 管理（Q3）

| 做法 | 既有部署能不能收到更新 | 代價 |
|---|---|---|
| **採用**：Wukong 寫入 agent 與 subagent 定義（跟現在每回合寫技能檔一樣會更新）；workspace/AGENTS.md 與歷代範本完全相同時先備份再換成新版，改過的不動，只在 log 提醒 | 會 | 要維護歷代範本的雜湊清單；定義寫在哪個目錄才會被三種執行模式都讀到，要在 Phase 1 確認 |
| 只改範本 | 不會。正式部署仍是 ac60ebe 版，會繼續叫模型去讀舊的技能路徑 | 使用者要自己改 |

## 3. 排程能力的 scope（Q4）

`[排程能力]` 的文字是固定的，只有 `--scope` 會隨回合變。一個 scope 對應一個 session，所以 scope 可以在建立 session 時寫進那個 session 的指令層。

| 做法 | 每回合的 user message | 風險 |
|---|---|---|
| **採用**：建立 session 時寫入 session instruction entry，說明文字改成 skill | 不含 scope | API 是 experimental。Phase 1 要先驗證：寫入後會持續存在、不進對話紀錄、模型讀得到；輪替出來的新 session 也要重寫一次 |
| 退路（Phase 1 驗證不過時採用） | 只多一行 scope | 穩定，但每回合仍有一行 |

## 4. 規劃偏好（Q2）

- **移除**：`planner_preferences` 與 Web 的偏好介面、API 都拿掉。
- **舊設定不報錯**：設定檔裡留著舊欄位時，讀取時忽略，不報錯。
- **理由**：正式部署的 `enabled` 是 false，偏好對象（角色與技能）的選用也都改由模型決定。

## 5. 不在本 issue 改的

- `[相關記憶]` 的召回注入：它是動態內容，而且記憶屬於「留在 Wukong」。
- `[其他回合正在執行]`：動態內容。
- `[無人值守排程]`：範圍第 3 項沒有列它。
- scope lease 的立即拒絕：範圍第 7 項只是評估，結論寫進計畫；要改成 inbox 排隊時，另外取得核准。
