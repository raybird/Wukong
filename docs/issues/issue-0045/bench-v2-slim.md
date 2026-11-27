# 效能對照：b65b487 與本分支（Task 6.2，SCN-001）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-62--效能對照)

2026-11-27 在本機以 Docker 實測。

## 結論

依 2026-11-27 修訂的 SCN-001 判定，**通過**。

| 條件 | b65b487 | 本分支 f0ce34a | 判定 |
|---|---|---|---|
| short 成功率 | 7/7 | 7/7 | 不低於 ✓ |
| tool 成功率（bench.txt 內容正確） | 7/7 | 7/7 | 不低於 ✓ |
| short 每回合 LLM 步數 | 3（planner 2＋主回合 1） | **1** | 較少 ✓ |
| 本機按需模式每回合啟動 opencode | 2 次 | **1 次** | ✓ |
| tool 每回合 LLM 步數（照實記錄） | 4（planner 2＋主回合 2） | 6～10，中位數 7 | 不設門檻 |

- tool 任務比 b65b487 多步，是 primary 不能改檔的代價，屬於刻意的設計（使用者 2026-11-26 為了 SCN-003 決定）。
  - 建立 bench.txt 一定要派給 fixer。primary 的步驟是：先讀一次看檔案在不在、派工、結束回覆，fixer 回報後再總結一次。fixer 自己要 3 步。
- 耗時：
  - short 任務的暖回合中位數從 31.9 秒降到 2.8 秒，CPU 時間從 4.4 秒降到 2.0 秒。
  - tool 任務的暖回合反而從 60.0 秒變成 91.4 秒（CPU 時間 4.4 → 3.2 秒）。多出的時間是 subagent 往返的模型延遲。
  - 同一組裡的耗時差異很大（b65b487 的 short 介於 9.9～74.6 秒），所以耗時只當參考，不當判準。

## 條件

- **限制**：`--cpus 1.5 --memory 3g --pids-limit 256`，本機按需模式（`WUKONG_AGENT_SERVER_URL` 為空），與 issue-0030 的 bench-turn.sh 相同。
- **模型**：兩個映像都用 `nvidia/nvidia/nemotron-3-ultra-550b-a55b`，寫在 `user.json`。憑證只有 NVIDIA 一筆。
- **初始狀態**：每個映像、每種任務各一份全新的合成狀態。同一個容器裡連跑 7 回合，第 1 回合算冷回合。
- **任務**：
  - short：「只回覆 OK 兩個字母」；
  - tool：「在目前的工作目錄建立 bench.txt，內容是 hello，然後讀出它的內容回覆給我」。
- **映像**：照 release workflow 的方式在本機建，見 [scripts/build-image.sh](scripts/build-image.sh)。
  - b65b487：#44 的合併版本；
  - ef3f74a：Task 6.1 完成時；
  - f0ce34a：加上「派工後就結束回覆」之後，最終版本。
- **執行**：
  - [scripts/bench-v2-slim.sh](scripts/bench-v2-slim.sh) 依序跑兩個映像，同一種任務先舊後新；
  - 每回合由 [scripts/bench-turn-steps.sh](scripts/bench-turn-steps.sh) 執行與記錄。
- **原始結果**：
  - [bench/b65b487-vs-ef3f74a.ndjson](bench/b65b487-vs-ef3f74a.ndjson)；
  - [bench/f0ce34a.ndjson](bench/f0ce34a.ndjson)（最終版本只重跑本分支；b65b487 沿用同日同條件的那組）。

## 步數與啟動次數怎麼量

`bench-turn-steps.sh` 是 bench-turn.sh 的延伸版。issue-0030 的原檔是當時的證據，所以不改它。

- **LLM 步數**：在 opencode.db 加一張 `bench_steps` 表，再加一個 `AFTER INSERT` trigger，每寫入一則 assistant 訊息就記一筆（session、agent）。
  - 為什麼不在回合結束後才去數：b65b487 的 planner 跑在暫時的 session，回合成功後就刪掉了，事後數不到。
  - 表要等 V2 建好 schema 才加得上去，所以第一個回合之前，先短暫啟動一次 `opencode serve` 讓它遷移。
- **opencode 啟動次數**：在 PATH 最前面放一個 opencode 的包裝程式，記下每次呼叫，只算 `serve`。wukong 是從 PATH 找 opencode 的。
- **反向自檢**：正式比較前，先在 b65b487 上確認量得到 planner 那一步。
  - 2 個 short 回合都是 3 步（`plan` 2、`build` 1），每回合啟動 2 次。
  - 回合結束後，`bench_steps` 記的 4 筆 planner 步數全都對不到還在的 session，`session_message` 裡只剩 2 則 assistant 訊息。
  - 也就是說，事後去數會漏掉 planner，用 trigger 才數得到。

## 經過

1. **ef3f74a（Task 6.1 完成時）**：short 每回合 1 步、啟動 1 次，達標。tool 7/7 成功，但步數 7～15（中位數 8）。
   - 原因：subagent 在背景執行，primary 派工後沒有結束回覆，而是反覆用 `read` 檢查 bench.txt，多數讀不到，每讀一次就是一次 LLM 呼叫。
   - 其中第 5、7 回合中途遇到 NVIDIA 的 429，V2 重試後成功，這兩回合的 15 步也包含重試。
2. **使用者 2026-11-27 選「消掉輪詢，修訂 tool 的步數條件」**：
   - primary 的派工說明加上「派工後就結束這次回覆、等它回報，不要在等待時反覆讀檔或查詢進度」（f0ce34a）；
   - SCN-001 的步數條件只適用 short 任務。
3. **f0ce34a**：tool 步數降到 6～10（中位數 7），primary 不再反覆讀檔。這次沒有遇到 429。

另外有一組無效的嘗試，沒有算進結果：

- 第一次正式比較用的是 `opencode/big-pickle`，因為 NVIDIA 稍早被限流。
- 但 28 回合都在約 2 秒內以「Free usage limit reached」（`provider.free-limit` 429）失敗，應該是 Task 6.1 的 18 回合用完了免費額度。
- 依 TBD-6 不輪換其他免費模型繞過額度；確認 NVIDIA 已恢復後改用 nemotron。
