# wukong-orchestrator

> 柱 3 ──「七十二變・分身」：角色調度引擎

把一項任務交給最合適的專家角色。兩段式：先讓 LLM 路由選角色，再以該角色卡執行。

## 五角色

`Explorer`（結構洞察）、`Oracle`（架構）、`Librarian`（文件）、`Fixer`（實作）、`Designer`（設計）。
角色卡以精簡常數內嵌（取自 tao-of-coding）。

## 公開 API（lib）

```rust
use wukong_orchestrator::{orchestrate, route, Role, Outcome};

// 兩段式：route（選角色）→ execute（以角色執行）
let outcome: Outcome = orchestrate(&backend, "fix the failing test").await?;
println!("{} -> {}", outcome.role.name(), outcome.text);

// 也可只取路由結果
let role: Role = route(&backend, "重構這個模組").await?;
```

`backend` 為任何實作 `wukong_gateway::backend::AiBackend` 的型別。
路由解析大小寫無關掃描角色名，解析失敗 fallback `Oracle`。

## 協作鏈（sequential collaboration chain）

單角色路由的延伸:一項任務可由數個角色**依序接力**,前一棒輸出累加成後一棒的 context。

```rust
use wukong_orchestrator::{plan_chain, orchestrate_chain, ChainOutcome, Role};

// planner 一次 LLM call 回有序角色清單(取代單角色 route)
let roles: Vec<Role> = plan_chain(&backend, "探查、實作、再寫文件").await?;

// 規劃後逐棒執行,回整鏈
let chain: ChainOutcome = orchestrate_chain(&backend, "探查、實作、再寫文件").await?;
println!("最終輸出:{}", chain.final_output());   // 最後一棒
for step in &chain.steps { println!("{} -> {}", step.role.name(), step.output); }
```

- **`plan_chain`/`planning_prompt`/`parse_chain`**:planner 對應物(對映 `route`/`routing_prompt`/`parse_role`)。`parse_chain` 依角色名出現位置排序、去重、**cap 3**;無角色 fallback `[Oracle]`。
- **`chain_context(&[Outcome])`**:把前序步驟渲染成 `[前序協作]` context 區塊(空 prior → 空字串)。
- **`ChainOutcome { steps }`** + `final_output()`:整鏈結果與最終輸出。
- 簡單任務 planner 回單角色 → 退化為長度 1 的鏈,行為與成本同今日。

## Demo bin

```bash
cargo run -p wukong-orchestrator --bin wukong-orchestrate -- \
  --agent-cmd "opencode run" "幫我把這段程式加上測試"
```

依賴方向：`wukong-orchestrator → wukong-gateway`（單向）。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md)。
