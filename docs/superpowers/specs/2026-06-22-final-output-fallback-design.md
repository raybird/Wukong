# 末棒空輸出回退與輸出要求(Final Output Fallback)設計

**日期:** 2026-06-22
**狀態:** 已核可,待轉實作
**前置:** 協作鏈(`2026-06-06-collaboration-chain-design.md`)、opencode session 控制(`2026-06-07-opencode-session-control-design.md`)。

## 背景與問題

`run_turn` 採「末棒即最終答覆」(`TurnOutput.text = 最後一棒 output`)。但 planner 不保證把「會回話」的角色排在最後;當末棒是 executor(典型 Fixer)時,它可能把工作全做在 tool call(改檔、跑指令),收尾不吐文字 → `resp.text` 為空。

後果(目前**無任何防護**):

- 使用者收到空白回應 —— Telegram 顯示「(無內容)」(`crates/wukong-telegram/src/dispatch.rs:226`)、Web 空泡泡、CLI 空行。
- `remember` 落盤 `Assistant: `(空)→ 污染記憶,下次 recall 撈到空殼。
- Web/Telegram 的 `record_chat` 同樣存入空 assistant → 污染對話歷史。

根因:「末棒=最終答覆」這個不變式,隱含假設末棒是「發言人」,但 executor 是「做事的人」,可能靜默收尾。

## 目標

1. **使用者永不收到純空白回應**(確定性保證)。
2. **不以空字串污染記憶與對話歷史**。
3. 降低 executor 靜默收尾的機率。

### 非目標(YAGNI)

- 不引入獨立「整合棒」(synthesizer),避免額外一次 backend call 與吃掉鏈長 cap 3。
- 不改 `run_turn` 簽章與 `TurnOutput` 形狀(下游四進入點零修改)。
- 不做鏈內動態重規劃。

## 設計

### ① 空輸出回退(治本,確定性)

於 `crates/wukong-runtime/src/turn.rs` 逐棒迴圈結束後、`remember` 與 return 之前,挑出「實際要回的那一棒」:

```rust
let last = prior.last().cloned()
    .unwrap_or(Outcome { role: Role::Oracle, output: String::new() });
let answer = if last.output.trim().is_empty() {
    prior.iter().rev()
        .find(|o| !o.output.trim().is_empty())
        .cloned()
        .unwrap_or(Outcome { role: last.role, output: "(本回合未產生文字輸出)".to_string() })
} else {
    last
};
```

- 回退順序:**末棒 → 最近一棒非空 → sentinel `(本回合未產生文字輸出)`**。
- `remember` 與 `TurnOutput` 改用 `answer.output` / `answer.role`。
- `answer.role` 報「實際被採用那棒」的角色(較誠實;若回退到 Explorer 的發現,就報 Explorer)。

一處改、四個進入點(CLI / Web / Telegram / Scheduler)同時受惠。

### ② 末棒輸出要求(降發生率,機率性)

`crates/wukong-runtime/src/persona.rs` 新增 helper,於 `run_turn` 的 `is_final` 區塊注入(與 `scheduling_capability_hint` 並列,僅最後一棒):

```rust
pub fn final_answer_directive() -> &'static str {
    "[輸出要求]\n無論你在過程中執行了多少工具操作(讀寫檔案、執行指令等),\
     最後務必用一段繁體中文文字向使用者清楚總結:你做了什麼、結果如何、有無注意事項。\
     不要只執行動作而不回覆文字。"
}
```

輔助棒 stateless,不注入。

### ①②分工

- ② 降低空白**發生機率**,但模型不保證遵守。
- ① 是**確定性安全網**,保證最終非空並治污染。
- 兩者搭配:② 治標、① 治本。

## 不變式變更(對 `2026-06-06-collaboration-chain-design.md` 的修訂)

| 原不變式(2026-06-06) | 修訂後(2026-06-22) |
|---|---|
| 最終輸出 = 最後一棒(spec §架構總覽、§cli run_turn 步驟 5) | 最終輸出 = 最後一棒**非空輸出**;全空回退 sentinel |
| 記憶只存最終輸出,不存每棒中間結果(spec §非目標) | 末棒空時,**回退存最近非空的中間棒輸出**(此時它即為使用者所見答覆) |

## 測試策略(TDD)

沿用既有 `MockBackend`(`run_streaming` 預設委派 `run`,可回空字串)。

- `run_turn_falls_back_when_final_output_empty`:腳本 `["explorer, fixer", "找到了根因", ""]` → `out.text == "找到了根因"`、`out.role == Explorer`、記憶含 `Assistant: 找到了根因`。
- `run_turn_all_empty_returns_sentinel`:腳本 `["fixer", ""]` → `out.text == "(本回合未產生文字輸出)"`。
- `run_turn_injects_final_answer_directive_into_final_step_only`:腳本 `["explorer, fixer", "e1", "f2"]` → `prompts[2]` 含 `[輸出要求]`、`prompts[1]` 不含。
- 既有 9 條 turn.rs 測試輸出皆非空 → 行為不變,全綠。

## 影響面

`run_turn` 為四進入點共用樞紐(GitNexus upstream 標 **CRITICAL**,32 受影響節點、13 直接)。但本變更**不動簽章**,語意只改「末棒空輸出」這條路徑,正常非空路徑完全不變 → 下游零修改、向後相容。實作後以 `gitnexus_detect_changes` 確認改動僅落在 `run_turn` 與 `persona`。
