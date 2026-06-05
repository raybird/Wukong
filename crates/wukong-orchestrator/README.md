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

## Demo bin

```bash
cargo run -p wukong-orchestrator --bin wukong-orchestrate -- \
  --agent-cmd "opencode run" "幫我把這段程式加上測試"
```

依賴方向：`wukong-orchestrator → wukong-gateway`（單向）。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md)。
