/// The Sun Wukong persona is managed globally at the system level via `SOUL.md`.
/// This constant is kept for documentation and fallback reference.
pub const WUKONG_PERSONA: &str = "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// 2026-11-26（#45 SCN-006）：Telegram 每回合附在使用者原文後的一行，後面接這個回合的產出
/// 目錄。規則本身（[`file_interaction_rules`]）在 agent 定義裡。`run_turn` 以它切出使用者原文
/// （2026-10-06 起的做法）：這一行只進 prompt，不進記憶與召回查詢。
pub const ARTIFACT_DIR_HEADER: &str = "[產出目錄]";

/// 檔案互動規則：寫在 primary agent 的定義裡；CLI backend 在回合帶有產出目錄時附在 prompt。
pub fn file_interaction_rules() -> &'static str {
    "[檔案互動規則]\n\
     使用者上傳的附件已是可修改的工作副本，不要修改 .wukong/uploads 內的原始檔。\
     訊息最後有 `[產出目錄] <路徑>` 這一行時，若使用者要求建立、轉換、修改或回傳檔案，\
     請將每個最終成品直接寫入該目錄；只把要回傳給使用者的成品放入該目錄。"
}

/// A directive appended to the final step instructing the agent to always
/// produce a textual summary — even when its work was done entirely via tool
/// calls — so an executor role never finishes silently and leaves the user
/// with a blank reply. Paired with the empty-output fallback in `run_turn`.
pub fn final_answer_directive() -> &'static str {
    "[輸出要求]\n\
     無論你在過程中執行了多少工具操作（讀寫檔案、執行指令等），\
     最後務必用一段繁體中文文字向使用者清楚總結：你做了什麼、結果如何、有無注意事項。\
     不要只執行動作而不回覆文字。"
}

/// Whether to advertise the Memoria CLI to the agent.
///
/// Off unless `WUKONG_MEMORIA_HINT` says otherwise, and it has to be an explicit
/// opt-in rather than auto-detection: this prompt is built inside
/// `wukong-web`/`wukong-telegram`/`wukong-schedulerd`, but the agent's shell runs
/// in the **opencode-server** container, which is the only one the Memoria
/// overlay wires up. Probing our own environment would answer the wrong
/// question, and telling the agent about a CLI that is not on its PATH is worse
/// than saying nothing — it would try, fail, and report the failure to the user.
///
/// `docker-compose.memoria.yml` sets this on the three services that build
/// prompts, so enabling the overlay is still a single decision for the operator.
pub fn memoria_hint_enabled() -> bool {
    matches!(
        std::env::var("WUKONG_MEMORIA_HINT")
            .unwrap_or_default()
            .trim(),
        "1" | "true" | "yes"
    )
}

/// Tell the agent it has a durable, cross-session memory CLI.
///
/// This exists because the Memoria overlay was wired at the environment layer
/// (CLI on PATH, `MEMORIA_HOME`, vector helper) but never at the prompt layer,
/// so the agent was never told it existed and the container's store stayed
/// empty — four days, zero agent-written memories. Availability is not
/// discoverability.
///
/// The division of labour matters and is stated explicitly below: `wukong-memory`
/// already records every turn automatically, so Memoria is *not* a transcript.
/// Without that line the agent would mirror the conversation into it and bury
/// the durable facts under chatter — the same dilution another Memoria-backed
/// deployment measured at 5:1.
pub fn memoria_capability_hint() -> &'static str {
    "[長期記憶能力]\n\
     你可以透過 `memoria` CLI 存取跨 session 的長期知識庫。它與本回合自動記錄的對話\
     記憶是兩套東西：對話已經被自動保存，**不需要**你再寫一次。\n\
     - 查詢既有結論：`memoria recall \"<查詢>\"`。回答涉及先前的決策、限制或踩過的坑時先查，\
     不要憑印象。\n\
     - 用到了就回報效用：`memoria feedback <recall_id> --score <0..1>`。那是這個系統唯一的\
     效用訊號來源，沒有回報它就學不到什麼有用。\n\
     - 記錄值得跨 session 留存的事：`memoria remember \"<內容>\"`。限於**決策與其理由、\
     學到的限制、非顯而易見的結論**；閒聊、可從程式碼或 git 讀出的事實、只在本回合有效的\
     內容都不要寫——寫進去會稀釋掉真正有用的部分。\n\
     - 寫入後向量索引不會自動更新，需要語意查詢才找得到時執行 `memoria-vector-sync`。\n\
     規則：查詢與寫入都由你自行判斷時機，不需要每回合都做，也不要為了做而做。"
}

/// 2026-10-20（issue-0012）：告訴 agent 還有哪些回合正在同一個工作目錄執行。
/// `scopes` 是其他持有 session lease 的 scope；排程回合以 `agent:schedule-<id>` 辨認。
pub fn concurrent_turns_notice(scopes: &[String], bin: &str) -> String {
    let mut s = String::from(
        "[其他回合正在執行]\n下列回合與你共用同一個工作目錄，此刻可能正在修改檔案：\n",
    );
    for scope in scopes {
        match scope.strip_prefix("agent:schedule-") {
            Some(id) => s.push_str(&format!("- 排程 {id}（{scope}）\n")),
            None => s.push_str(&format!("- 對話 {scope}\n")),
        }
    }
    s.push_str(&format!(
        "不要修改或提交不是本回合造成的變更；遇到預期外的檔案變動，先說明再處理。\
         想知道某個排程在做什麼，可以執行 `{bin} schedule list`。"
    ));
    s
}

/// Resolve the `wukong` binary name used inside the scheduling capability hint.
/// Defaults to `wukong` (assumes it is on PATH in the agent runtime); override
/// with `WUKONG_BIN` (e.g. an absolute path) when it is not.
pub fn scheduling_bin() -> String {
    std::env::var("WUKONG_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "wukong".to_string())
}

/// 2026-11-26（#45 SCN-007）：session 指令層放 scope 的 entry key。
pub const SCOPE_INSTRUCTION_KEY: &str = "wukong.scope";

/// 排程能力的固定說明：寫在 primary agent 的定義裡（[`crate::agent_assets`]），不隨回合改變。
/// 排程的擁有者取自 [`current_scope_instruction`]：V2 backend 放在 session 指令層，CLI backend
/// 跟這段說明一起附在 prompt（[`scheduling_capability_hint`]）。
pub fn scheduling_capability_guide(bin: &str) -> String {
    format!(
        "[排程能力]\n\
         你具備為使用者建立與管理定時排程（cron）的真實能力。當使用者要求「定時、每天、每隔、提醒、排程、固定時間執行」之類任務時，\
         請直接在終端機執行下列指令完成設定——這是可用的內建功能，不要拒絕，也不要只用文字描述而不執行：\n\
         - 新增定時對話：`{bin} schedule add-turn --name \"<名稱>\" --cron \"<分 時 日 月 週>\" --scope \"<目前的 scope>\" --prompt \"<到時要執行的提示詞>\"`\n\
         - 指定排程使用的模型：新增時加 `--model \"<provider/model>\"`；修改既有排程用 `{bin} schedule set-model --id <id> --model \"<provider/model>\"`，省略 `--model` 就改回使用預設模型\n\
         - 列出所有排程：`{bin} schedule list`\n\
         - 立即觸發一次：`{bin} schedule trigger --id <id>`\n\
         - 查看排程的執行結果：`{bin} schedule runs --id <id>`\n\
         - 停用／啟用／刪除：`{bin} schedule disable|enable|rm --id <id>`\n\
         規則：cron 為標準 5 欄位（分 時 日 月 週）。`<目前的 scope>` 是 [目前的 scope] 寫的值；除非使用者另外指定 scope，否則一律用它。\
         `--scope` 是排程的擁有者，執行結果會回送給它；排程回合本身在獨立的記憶 scope 執行，\
         所以使用者問起排程回報過什麼時，用上面的查看指令取得，不要憑記憶回答。\
         建立完成後，用一句話回報排程名稱、cron 設定與用途即可。"
    )
}

/// 這個對話的 scope（記憶與排程的擁有者）。
pub fn current_scope_instruction(scope: &str) -> String {
    format!("[目前的 scope]\n這個對話的 scope 是 `{scope}`，建立排程時 `--scope` 填這個值。")
}

/// 不支援 session 指令層的 backend（CLI）每回合附在 prompt 的排程能力：固定說明加上 scope。
pub fn scheduling_capability_hint(scope: &str, bin: &str) -> String {
    format!(
        "{}\n\n{}",
        scheduling_capability_guide(bin),
        current_scope_instruction(scope)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_answer_directive_requires_text_summary() {
        let d = final_answer_directive();
        assert!(d.contains("[輸出要求]"));
        assert!(d.contains("總結"));
    }

    #[test]
    fn scheduling_hint_embeds_scope_bin_and_command() {
        let hint = scheduling_capability_hint("project:Wukong", "wukong");
        assert!(hint.contains("[排程能力]"));
        assert!(hint.contains("schedule add-turn"));
        assert!(hint.contains("--scope \"<目前的 scope>\""));
        assert!(hint.contains("[目前的 scope]\n這個對話的 scope 是 `project:Wukong`"));
        assert!(hint.contains("`wukong schedule list`"));
    }

    // issue-0014 SCN-005：排程回合不寫進使用者記憶，對話中改用 CLI 查執行結果。
    #[test]
    fn scheduling_hint_lists_run_history_and_explains_owner_scope() {
        let hint = scheduling_capability_hint("user:tg-42", "wukong");
        assert!(hint.contains("`wukong schedule runs --id <id>`"));
        assert!(hint.contains("`--scope` 是排程的擁有者"));
    }

    // issue-0026 SCN-003：對話中也能指定與修改排程的模型。
    #[test]
    fn scheduling_hint_lists_model_commands() {
        let hint = scheduling_capability_hint("user:tg-42", "wukong");
        assert!(hint.contains("--model \"<provider/model>\""), "{hint}");
        assert!(
            hint.contains("`wukong schedule set-model --id <id> --model \"<provider/model>\"`"),
            "{hint}"
        );
        assert!(hint.contains("省略 `--model`"), "{hint}");
    }

    #[test]
    fn scheduling_hint_honors_custom_bin() {
        let hint = scheduling_capability_hint("global", "/opt/wukong");
        assert!(hint.contains("`/opt/wukong schedule add-turn"));
    }

    #[test]
    fn scheduling_bin_defaults_to_wukong() {
        // No env override in test => default name.
        std::env::remove_var("WUKONG_BIN");
        assert_eq!(scheduling_bin(), "wukong");
    }
}
