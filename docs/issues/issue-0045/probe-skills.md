# 探測：技能的探索路徑與使用（Task 1.2）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-12--技能的探索路徑與使用)

2026-11-26 在隔離容器實測。環境與 [probe-agents.md](probe-agents.md#環境) 相同：

- 映像 `wukong-issue44:v2-local`，opencode v2.3.2；
- 沒有憑證；
- 模型是預設免費模型 `opencode/big-pickle`，共 2 個回合；
- 只送合成提示。

## 結論

- **Phase 2 採用的路徑**：工作區的 `.opencode/skills/{名稱}/SKILL.md`。這和 agent 定義採用的 `.opencode/agents/` 在同一個目錄底下。
- **能讀到的位置**：V2 會從以下 8 個位置讀技能。
  - 工作區：`.opencode/skills`、`.opencode/skill`、`.agents/skills`、`.claude/skills`；
  - 使用者層：`~/.config/opencode/skills`、`~/.config/opencode/skill`、`~/.agents/skills`、`~/.claude/skills`。
  - 這 8 個位置是 2026-11-26 稍早的手動探測各放一個 SKILL.md 確認的。這次的腳本只驗證 Phase 2 要用的 `.opencode/skills`。
- **讀不到的位置**：Wukong 現在用的 `.wukong/skills/superpowers` 不在 V2 的探索範圍裡，放在那裡的技能不會出現在 `/api/skill`。
- **2026-11-24 的觀察**：當時「專案技能沒有出現」，原因是 server 剛啟動時的競態，不是路徑不對。
  - 剛就緒時的第一個清單請求看不到自訂項目，幾秒後才完整，和 agent 清單一樣（見 probe-agents.md）。
  - 腳本因此輪詢到自訂技能出現為止。
- **Superpowers 原樣可用**：八個 Superpowers 技能原樣複製過去，全部出現在 `/api/skill`，name 與 description 都被讀到。
  - frontmatter 只有 `name` 與 `description`，不用改。
  - `autoinvoke` 欄位是空的。沒有設定時，模型仍可以自己載入，見下一點。
- **模型自己載入**：使用者的問題符合技能的 description 時，模型會呼叫內建的 `skill` 工具載入技能內文，並照著做。
  - 這次的答案只寫在技能內文裡（`AUTO-7Q3`），回覆正確，代表技能確實被讀過。
  - 事件的順序是：
    - `session.tool.input.started`，`data.name` 為 `skill`；
    - `session.tool.called`，`data.input` 為 `{"id": "wk-probe-auto"}`；
    - `session.tool.success`，`data.content` 是技能內文，包在 `skill_content` 標籤裡。
  - 訊息列表裡，那則 assistant 訊息有一個 type 為 `tool`、name 為 `skill` 的部分。
- **prompt 附上技能**：`POST /api/session/{id}/prompt` 帶 `"skills": [{"id": "wk-probe-attach"}]` 時，V2 把技能內文附在這則 user 訊息上，模型照著做。
  - 這個回合沒有呼叫工具。
  - `session.inbox.enqueued` 事件的 `data.item.payload.skills` 帶著技能的 id、name 與內文；訊息列表裡那則 user 訊息的 `skills` 欄位也一樣。
  - 附上的內文會寫進對話紀錄。每回合都附，對話紀錄每回合都會多一份技能內文。
- **對 SCN-005 的意思**：
  - 技能寫在 `.opencode/skills/` 之後，交給模型依 description 自己載入就夠了。Wukong 不必再把技能清單或路徑寫進 prompt，也不必每回合附上。
  - 要確認是否用到技能，看事件裡 name 為 `skill` 的工具呼叫，以及它的 `input.id`。
- **另一個入口**：`POST /api/experimental/session/{id}/skill` 帶 `{"id": …, "resume": …}`，可以直接啟用技能。OpenAPI 的說明是：附加一則 skill 訊息，然後繼續執行。這次沒有測，Phase 2 不需要它。

## 命令

```bash
work=$(mktemp -d "$SCRATCH/p45k.XXXX")
bash docs/issues/issue-0045/scripts/probe-skills.sh \
    wukong-issue44:v2-local "$work" "$SCRATCH/p45-empty-auth.json" opencode/big-pickle
```

- 腳本：[scripts/probe-skills.sh](scripts/probe-skills.sh)。
- 腳本會在工作區放三種技能：
  - 從 `crates/wukong-skills/assets/superpowers/` 原樣複製的八個技能；
  - 兩個探測用技能 `wk-probe-auto`、`wk-probe-attach`；
  - 放在 `.wukong/skills/superpowers/` 的對照組 `wk-hidden`。
- 回合由 [scripts/v2turn.py](scripts/v2turn.py) 送出。`--skill` 參數會把技能附在 prompt 上。

## 輸出

```text
== 1. /api/skill（常駐 server，cwd 是工作區）
  opencode  name=OpenCode  autoinvoke=None  path=skill://opencode/SKILL.md  desc=455 chars
  report  name=Report  autoinvoke=None  path=skill://report/SKILL.md  desc=168 chars
  browser  name=Browser  autoinvoke=None  path=skill://browser/SKILL.md  desc=197 chars
  brainstorming  name=brainstorming  autoinvoke=None  path=/p/ws/.opencode/skills/brainstorming/SKILL.md  desc=198 chars
  executing-plans  name=executing-plans  autoinvoke=None  path=/p/ws/.opencode/skills/executing-plans/SKILL.md  desc=104 chars
  receiving-code-review  name=receiving-code-review  autoinvoke=None  path=/p/ws/.opencode/skills/receiving-code-review/SKILL.md  desc=234 chars
  requesting-code-review  name=requesting-code-review  autoinvoke=None  path=/p/ws/.opencode/skills/requesting-code-review/SKILL.md  desc=107 chars
  systematic-debugging  name=systematic-debugging  autoinvoke=None  path=/p/ws/.opencode/skills/systematic-debugging/SKILL.md  desc=91 chars
  test-driven-development  name=test-driven-development  autoinvoke=None  path=/p/ws/.opencode/skills/test-driven-development/SKILL.md  desc=79 chars
  verification-before-completion  name=verification-before-completion  autoinvoke=None  path=/p/ws/.opencode/skills/verification-before-completion/SKILL.md  desc=225 chars
  wk-probe-attach  name=wk-probe-attach  autoinvoke=None  path=/p/ws/.opencode/skills/wk-probe-attach/SKILL.md  desc=54 chars
  wk-probe-auto  name=wk-probe-auto  autoinvoke=None  path=/p/ws/.opencode/skills/wk-probe-auto/SKILL.md  desc=48 chars
  writing-plans  name=writing-plans  autoinvoke=None  path=/p/ws/.opencode/skills/writing-plans/SKILL.md  desc=84 chars
PASS /api/skill lists brainstorming with its description
PASS /api/skill lists executing-plans with its description
PASS /api/skill lists receiving-code-review with its description
PASS /api/skill lists requesting-code-review with its description
PASS /api/skill lists systematic-debugging with its description
PASS /api/skill lists test-driven-development with its description
PASS /api/skill lists verification-before-completion with its description
PASS /api/skill lists writing-plans with its description
PASS /api/skill lists wk-probe-auto with its description
PASS /api/skill lists wk-probe-attach with its description
PASS /api/skill does not list the skill under .wukong/skills/superpowers
== 2. 模型自己決定載入技能
  assistant part type=tool {"name": "skill"}
PASS auto: the reply carries the passphrase that only the skill body has (AUTO-7Q3 [model=opencode/big-pickle])
PASS auto: the events name the loaded skill
== 3. prompt 附上 skills
PASS attach: the attached skill is followed (ATTACH-5K2 Hello there, how are you? [model=opencode/big-pickle])
PASS attach: the events name the attached skill
fails=0
```

- 這份輸出省略了 v2turn.py 每回合印的 JSON 摘要行。
- 清單最前面的 opencode、report、browser 是 V2 內建的技能，路徑是 `skill://`。
