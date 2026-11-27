# wukong-orchestrator

> 柱 3 ──「七十二變・分身」：五個專家角色

定義五個專家角色：`Explorer`（結構洞察）、`Oracle`（架構）、`Librarian`（文件）、`Fixer`（實作）、`Designer`（設計）。
角色卡以精簡常數內嵌（取自 tao-of-coding）。

2026-11-26（#45）起，路由、planner 與協作鏈都已移除：`wukong-runtime` 把每個角色寫成 OpenCode V2 的 subagent
定義（工作區的 `.opencode/agents/`），由 primary agent `wukong` 自己決定要不要派工、派給誰。

## 公開 API（lib）

```rust
use wukong_orchestrator::Role;

for role in Role::all() {
    println!("{}：{}", role.name(), role.description());
    let card: &str = role.card(); // subagent 定義的內文
}
```

- `name()`：subagent 名稱（`explorer`、`oracle`、`librarian`、`fixer`、`designer`），也是入口顯示「悟空·fixer」的依據。
- `description()`：subagent 定義的 description，V2 用它讓 primary agent 判斷派給誰。
- `card()`：角色卡。

沒有內部依賴。

早期的兩段式路由與協作鏈設計見 [`docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-orchestrator-design.md)（歷史文件）。
