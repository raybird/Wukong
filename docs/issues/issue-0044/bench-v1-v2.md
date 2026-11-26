# Task 6.2：V1 與 V2 的版本對照

> ← 回到 [README](README.md)｜[implementation-plan.md](implementation-plan.md) Task 6.2

- **日期**：2026-11-25 15:16～15:39（台北），主機 flashaim
- **映像**：
  - `ghcr.io/raybird/wukong:v0.25.1`（opencode 1.19.6）
  - 本分支建的 `wukong-issue44:v2-local`（@opencode/cli 2.3.2）
- **工具**：issue-0030 的 [bench-turn.sh](../issue-0030/scripts/bench-turn.sh)，由 [scripts/bench-v1-v2.sh](scripts/bench-v1-v2.sh) 呼叫
  - 容器限制與正式部署相同：1.5 CPU、3 GiB、256 PID
  - entrypoint 只跑一次，之後在同一個容器裡連跑回合：第 1 回合算冷，之後算暖
  - 任務 short 只回 OK；tool 建立並讀回 `bench.txt`
- **模型**：兩邊都用使用者的 NVIDIA 模型 `nvidia/nvidia/nemotron-3-ultra-550b-a55b`，寫在 `user.json`
- **原始資料**：[bench/](bench/)
  - `round1.ndjson`：先 V1 後 V2，各 3 回合
  - `round2.ndjson`（short）與 `round2-tool.ndjson`：順序反過來，先 V2 後 V1，各 4 回合
  - `attempt0-default-model.ndjson`：失敗樣本，見最後一節

## 與 issue-0030 的差異

- **RUN_DIR 是全新的合成狀態**：issue-0030 用的是正式部署的副本。這次如果也用副本，記憶會被召回進 prompt 送給外部模型，違反「不把正式對話送給外部模型」。兩個映像都從空的 opencode.db 起算，所以 V2 的數字不含遷移的成本；遷移本身見 [rehearsal.md](rehearsal.md)。
- **`bench-turn.sh` 補上 `OPENCODE_CONFIG`**：回合在 entrypoint 之外執行，原本沒有這個變數，`user.json` 不會生效，回合會用預設的免費模型。現在補上與 entrypoint 交給正式服務相同的值。issue-0030 當時就是用預設模型，不受影響。

## 結果

| | V1 short | V2 short | V1 tool | V2 tool |
|---|---|---|---|---|
| 成功 | 7/7 | 7/7 | 7/7 | 7/7 |
| 冷回合耗時（秒） | 18.8、13.5 | 10.9、20.1 | 19.6、16.6 | 42.9、19.0 |
| 暖回合耗時（秒） | 9.5、7.3、12.2、16.1、18.9 | 13.2、14.0、32.1、7.6、12.8 | 13.6、13.9、26.3、20.1、16.7 | 25.0、27.0、23.0、21.9、19.0 |
| 冷回合 CPU（秒） | 9.7、11.1 | 4.6、4.8 | 7.0、6.0 | 5.0、4.7 |
| 暖回合 CPU 平均（秒） | 5.1 | 4.3 | 6.0 | 4.3 |
| 暖回合被 throttle 的時間平均（毫秒） | 281 | 1,163 | 281 | 1,096 |
| 記憶體峰值（MiB） | 1,032 | 589～622 | 625～800 | 557～593 |
| opencode.db 冷回合增量 | 0.99 MB | 14.1 MB | 1.57～1.95 MB | 14.1 MB |
| opencode.db 暖回合增量 | 20～45 KB | 4 KB | 48～342 KB | 4～12 KB |

tool 任務的每一回合，`bench.txt` 的內容都是 hello（`bench_file=ok`）。

## 解讀

- **耗時：看不出 V2 退步，兩邊都主要取決於上游。**
  - 第一輪看起來 V2 比較慢：short 暖回合 13～14 秒對 7～9.5 秒，tool 25～27 秒對 14 秒。
  - 順序反過來的第二輪，兩邊的範圍互相重疊：short 是 7.6～32.1 秒對 12.2～18.9 秒，tool 是 19.0～23.0 秒對 16.7～26.3 秒。
  - 第一輪 opencode.db 裡每一步模型呼叫的時間，V2 也比較長。但兩邊每一步的 input 加 cache token（約 8～13k）與 reasoning token 都相近，所以不是 V2 送了比較大的 prompt。
  - 同一個模型在不同分鐘取樣，差異就有這麼大。要分出 V2 每一步本身有沒有額外成本，需要固定回應的上游；這次沒有做，列為未知。
- **CPU：V2 比較少。** 暖回合約少 15～30%；冷回合 V2 是 4.6～5.0 秒，V1 是 6.0～11.1 秒。
- **throttling：V2 比較多，原因未知。** 總 CPU 比較少，被 throttle 的時間反而約是 V1 的 4 倍，每回合約 1.1 秒對 0.28 秒，表示 V2 的 CPU 用量集中在比較短的時間內。推測是 V2 在 bun 上啟動與執行時同時用多個執行緒，但這次沒有驗證。在正式部署的 1.5 CPU 限制下，這約 1 秒只會延長回合，不會造成失敗。
- **記憶體：V2 比較少。** 峰值約 560～620 MiB 對 625～1,030 MiB。`memory.peak` 在容器裡無法重設（唯讀），所以這是每份 RUN_DIR 所有回合裡的最大值。
- **opencode.db：V2 第一次多 14 MB，之後比較少。**
  - 14 MB 幾乎全是 `kv` 表裡的 `models-dev:v2:catalog`，也就是 models.dev 模型目錄的快取，只寫一次。
  - 之後每回合只增加 4～12 KB，V1 是 20～342 KB。
- **memory.db 增量都是 0**：bench 容器沒有設定 `WUKONG_MEMORY_DB`，`/data/memory.db` 不是 Wukong 實際使用的那份，與版本無關。issue-0030 的數字也一樣是 0。

## 失敗樣本：沒有套用 user.json 的第一輪

`attempt0-default-model.ndjson` 是補上 `OPENCODE_CONFIG` 之前跑的，跑到一半就停掉。

- 兩個映像都沒有用 `user.json` 的模型，而是用預設的免費模型。
- V1 short 三回合成功，每回合 62～89 秒。
- V2 short 第 1、3 回合收到真實的 `provider.free-limit` 429：Wukong 回報「上游模型錯誤（上游限流）」，分別在 5.9 秒、3.8 秒內結束，沒有變成逾時。
- V2 short 第 2 回合成功，但回答離題。
- 這一輪不算進上面的表。
