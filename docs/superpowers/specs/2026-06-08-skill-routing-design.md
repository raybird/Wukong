# 技能路由 (Skill Routing) 設計

**日期:** 2026-06-08  
**狀態:** 已核可方向，待轉實作計畫  
**前置:** v0.5.0 協作鏈 (`plan_chain` / `run_turn`)、opencode session 控制、既有五角色 `Role`。

## 背景

Wukong 目前已完成「角色路由」與「角色協作鏈」：每回合先由 planner 選出最多三個角色，再依序執行。v0.5.0 的協作鏈 spec 明確把技能路由列為非目標，因此目前 runtime 只知道 Explorer / Oracle / Librarian / Fixer / Designer，不知道 `brainstorming`、`test-driven-development`、`systematic-debugging` 等 Superpowers 技能流程。

Tao of Coding 的現行做法提供可參考方向：以 `SKILL.md` 的「技能路由表」作為單一事實來源，將 selected Superpowers 技能 vendored 到 `references/superpowers/`，並用 `sync-superpowers.sh` 從上游 `obra/superpowers` 同步與記錄來源 commit。不過 Tao 的調度由「當前 agent 本體」讀取 markdown 後執行；Wukong 是 Rust CLI runtime，因此需要把相同概念轉成可測、可注入 prompt 的程式結構。

## 目標

- Wukong repo 內建本地化 Superpowers 技能庫，第一版只選入 runtime 會用到的核心技能。
- planner 從「只規劃角色鏈」升級為「規劃角色 + 技能」的有序鏈。
- 每個執行步驟可把對應 `SKILL.md` 內容注入 prompt，讓底層 opencode 不只知道技能名稱，也看得到流程規範。
- Superpowers 更新方式沿用 Tao 的 `sync-superpowers.sh` 思路：可 dry-run、可指定 commit/tag、可更新 `SOURCE.md`。
- 保留既有 `plan_chain`、`orchestrate_chain` 與 `TurnOutput` 的形狀，降低破壞範圍。

## 非目標

- 不做平行 fan-out；仍維持 v0.5.0 協作鏈的 sequential 執行模式。
- 不實作宿主技能安裝或 symlink；Wukong CLI runtime 使用 repo 內建技能庫。
- 不遞迴注入技能 references；第一版只注入每個技能目錄下的 `SKILL.md`。
- 不讓使用者在 CLI 上手動指定技能；第一版由 planner 自動選。
- 不新增每技能不同模型分配。

## 技能範圍

第一版選入 Tao Phase 1 runtime 技能：

| 技能 | 主責角色 | 協作角色 | 觸發任務 |
| :--- | :--- | :--- | :--- |
| `brainstorming` | Oracle | Designer | 創意發想、需求澄清、方案比較 |
| `writing-plans` | Oracle | Librarian | 多步驟實作計畫撰寫 |
| `executing-plans` | Explorer | Fixer | 依既有計畫批次執行 |
| `test-driven-development` | Fixer | Oracle | 新功能或 bugfix 的測試先行實作 |
| `systematic-debugging` | Fixer | Explorer | 錯誤追因、根因定位 |
| `verification-before-completion` | Fixer | Oracle | 宣告完成、commit、PR 前驗證 |
| `requesting-code-review` | Librarian | Oracle | 發 PR 或完成任務後請求審查 |
| `receiving-code-review` | Fixer | Librarian | 接收審查意見並分級處理 |

以下 Tao 調度技能暫不進入 Wukong runtime catalog：

- `subagent-driven-development`
- `dispatching-parallel-agents`

原因是它們描述「當前 agent 如何使用宿主 subagent」，而 Wukong runtime 目前只透過單一 `AiBackend` 驅動 opencode，沒有原生子代理 API。

## 架構

新增 crate：`wukong-skills`。

```
crates/wukong-skills/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── catalog.rs
└── assets/
    └── superpowers/
        ├── brainstorming/SKILL.md
        ├── writing-plans/SKILL.md
        ├── executing-plans/SKILL.md
        ├── test-driven-development/SKILL.md
        ├── systematic-debugging/SKILL.md
        ├── verification-before-completion/SKILL.md
        ├── requesting-code-review/SKILL.md
        ├── receiving-code-review/SKILL.md
        └── SOURCE.md
```

`wukong-skills` 依賴 `wukong-orchestrator` 取得 `Role`。`wukong-orchestrator` 不反向依賴 `wukong-skills`，避免循環；實作上由 `wukong-cli` 將技能 catalog 資訊交給 orchestrator 的規劃 prompt，並在執行階段讀取技能內容。

核心型別：

```rust
pub enum SkillId {
    Brainstorming,
    WritingPlans,
    ExecutingPlans,
    TestDrivenDevelopment,
    SystematicDebugging,
    VerificationBeforeCompletion,
    RequestingCodeReview,
    ReceivingCodeReview,
}

pub struct SkillSpec {
    pub id: SkillId,
    pub name: &'static str,
    pub description: &'static str,
    pub primary_role: Role,
    pub collaborator_role: Option<Role>,
    pub content: &'static str,
}
```

技能內容使用 `include_str!` 內嵌，讓 release binary 不需要在 runtime 尋找外部檔案。同步腳本更新 assets 後，重新編譯即可帶入新版技能。

## Orchestrator 擴充

保留既有 API：

- `planning_prompt(task) -> String`
- `parse_chain(response) -> Vec<Role>`
- `plan_chain(backend, task) -> Result<Vec<Role>, OrchestratorError>`

新增技能規劃 API：

```rust
pub struct SkillRouteOption {
    pub skill_name: &'static str,
    pub description: &'static str,
    pub primary_role: Role,
    pub collaborator_role: Option<Role>,
}

pub struct PlannedStep {
    pub role: Role,
    pub skill_name: Option<String>,
}

pub fn skill_planning_prompt(task: &str, skills: &[SkillRouteOption]) -> String;
pub fn parse_skill_chain(response: &str) -> Vec<PlannedStep>;
pub async fn plan_skill_chain(
    backend: &impl AiBackend,
    task: &str,
    skills: &[SkillRouteOption],
) -> Result<Vec<PlannedStep>, OrchestratorError>;
```

`skill_name` 先用 `String` 而不是直接依賴 `SkillId`，讓 orchestrator 保持不依賴 `wukong-skills`。`wukong-cli` 負責將 `skill_name` 轉成 `SkillId` 並查 catalog；無法辨識時，該步驟退化為只有角色。

### Planner 輸出格式

`skill_planning_prompt` 要求模型只回每行一個步驟：

```text
<role>|<skill-or-none>
```

範例：

```text
oracle|brainstorming
fixer|test-driven-development
```

解析規則：

- `role` 使用既有五角色名稱，大小寫不敏感。
- `skill` 使用 catalog 名稱，大小寫不敏感；`none`、空字串或未知技能都視為 `None`。
- 最多取前三個有效角色步驟。
- 完全無有效角色時 fallback 為 `[PlannedStep { role: Oracle, skill_name: None }]`。
- 若模型回自然語言，解析器仍以「掃描角色名稱出現位置」作為備援，並將技能設為 `None`。

## CLI 執行流程

`wukong-cli::run_turn` 從現行流程：

```text
recall → plan_chain → execute×N → remember
```

改為：

```text
recall → plan_skill_chain → execute×N(skill-aware prompt) → remember
```

執行規則：

1. 先 recall 一次，供整條鏈共用。
2. 由 `wukong_skills::catalog::route_options()` 提供技能路由表給 `plan_skill_chain`。
3. 每個 `PlannedStep` 觸發 `on_role(role)`，維持目前 UI/Telegram/Web 的角色進度顯示相容。
4. `skill_name` 可解析成 catalog 技能時，將技能內容注入 prompt；不可解析時只注入人格、角色、記憶與任務。
5. session 隔離沿用現況：只有最後一棒帶入並更新 scope 的 opencode `session_id`。
6. 記憶仍只存 User input 與最後一棒 Assistant output，不存中間棒或技能名稱。

`TurnOutput` 第一版維持不變：

```rust
pub struct TurnOutput {
    pub role: Role,
    pub text: String,
}
```

不新增 `skill` 欄位，避免影響 Telegram / Web / REPL 呼叫端。若未來 UI 需要顯示技能，可另開 `TurnTrace` 或 progress callback。

## Prompt 組裝

`persona::build_prompt` 新增技能感知版本，保留舊函式給測試與相容使用：

```rust
pub fn build_prompt_with_skill(
    role: Role,
    skill: Option<&SkillSpec>,
    hits: &[RecallHit],
    input: &str,
) -> String;
```

輸出結構：

```text
<WUKONG_PERSONA>

<role.card()>

[技能規範]
你必須遵循 `<skill-name>` 的流程。以下是技能文件：
<SKILL.md content>

<memory + input block>
```

無技能時省略整個 `[技能規範]` 區塊。技能內容可能較長，因此第一版限制每棒最多一個技能，並不注入 reference 檔。

## Superpowers 同步

新增維護腳本：

```text
scripts/sync-superpowers.sh <commit-or-tag> [--dry-run] [--repo <repo_url>]
```

流程沿用 Tao：

1. 建立暫存 git repo。
2. fetch 指定 upstream ref。
3. 複製 selected skills 到 staging。
4. 產生 `assets/superpowers/SOURCE.md`，記錄 upstream repo、MIT license、resolved commit、import date。
5. `--dry-run` 顯示目的地與 staged 差異，不寫入。
6. 正式同步時先備份舊目錄，再原子替換。
7. 驗證每個 selected skill 都有 `SKILL.md`。

selected skills 清單與 `SkillId` 必須一致。若未來新增技能，需要同時更新 enum、catalog、同步腳本與測試。

## 錯誤處理與 fallback

- planning backend 失敗：沿用 `OrchestratorError`，整回合失敗。
- parser 無法解析角色：fallback Oracle + 無技能。
- parser 找到未知技能：保留角色，技能降級為 `None`。
- 本地 catalog 缺技能內容：測試階段視為錯誤；runtime 不應發生，因內容用 `include_str!`。
- 同步腳本缺上游技能檔：腳本失敗，不覆蓋既有 assets。

## 測試策略

`wukong-skills`：

- `SkillId::as_str()` 與 `SkillId::from_name()` roundtrip。
- catalog 包含八個 runtime 技能。
- 每個 `SkillSpec.content` 非空且含標題或流程關鍵字。
- `route_options()` 的角色對應與本 spec 表格一致。

`wukong-orchestrator`：

- `skill_planning_prompt` 列出角色、技能與任務。
- `parse_skill_chain("fixer|test-driven-development")` 解析 role + skill。
- 多行輸出保持順序並 cap 3。
- `none` / 空技能 / 未知技能退化為 `None`。
- 全無有效角色 fallback Oracle。

`wukong-cli`：

- 單角色 + 技能腳本回應時，execute prompt 含 `[技能規範]` 與技能內容。
- 未知技能時仍可執行角色，不注入技能區塊。
- 多角色技能鏈仍只最後一棒帶 session。
- 記憶只保存最後一棒輸出。
- `TurnOutput` 維持最後一棒 role + text。

同步腳本：

- `--dry-run` 不改檔，並輸出 resolved commit。
- staging 缺任一 `SKILL.md` 時失敗。
- 正式同步會寫入 `SOURCE.md`。

## 驗收標準

1. `cargo test` 全綠。
2. `cargo clippy --all-targets -- -D warnings` 乾淨。
3. `scripts/sync-superpowers.sh <ref> --dry-run` 可成功列出差異。
4. 以 mock backend 驗證：planner 回 `fixer|test-driven-development` 時，execute prompt 內含 Fixer 角色卡與 TDD 技能文件。
5. README roadmap 從「技能路由(後續)」更新為技能路由已完成，並註明平行多角色調度仍為後續。

## 風險與取捨

- **Prompt 成本增加**：`SKILL.md` 可能很長。第一版用「每棒最多一技能、不注入 references」控成本。
- **模型輸出格式不穩**：提供嚴格 `<role>|<skill>` 格式並保留角色掃描 fallback。
- **技能版本漂移**：以 `SOURCE.md` 記錄來源 commit，並要求同步腳本與 catalog 清單一致。
- **UI 不顯示技能**：第一版保留 `TurnOutput` 形狀，只顯示角色；若要顯示技能，另開 callback 或 trace 型別，避免本次擴張範圍。
