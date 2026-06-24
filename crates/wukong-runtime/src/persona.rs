use wukong_memory::RecallHit;
use wukong_orchestrator::Role;
use wukong_skills::SkillSpec;

/// The Sun Wukong persona is managed globally at the system level via `SOUL.md`.
/// This constant is kept for documentation and fallback reference.
pub const WUKONG_PERSONA: &str = "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// Build the execution prompt: role card + (memory context + input).
/// The memory/input section reuses gateway's compose_prompt.
pub fn build_prompt(role: Role, hits: &[RecallHit], input: &str) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    format!("{}\n\n{body}", role.card())
}

pub fn build_prompt_with_skill(
    role: Role,
    skill: Option<&SkillSpec>,
    hits: &[RecallHit],
    input: &str,
) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    let mut prompt = role.card().to_string();
    if let Some(skill) = skill {
        prompt.push_str(&format!(
            "\n\n[技能規範指引]\n你必須遵循 `{}` 的流程。請先閱讀並遵循專案中的技能規範文件：\n路徑：crates/wukong-skills/assets/superpowers/{}/SKILL.md",
            skill.name, skill.name
        ));
    }
    prompt.push_str("\n\n");
    prompt.push_str(&body);
    prompt
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

/// An always-on capability block telling the agent it can create and manage
/// scheduled (cron) jobs by shelling out to the `wukong schedule` CLI. `scope`
/// is the current turn's memory scope so scheduled turns land in the same place.
pub fn scheduling_capability_hint(scope: &str, bin: &str) -> String {
    format!(
        "[排程能力]\n\
         你具備為使用者建立與管理定時排程（cron）的真實能力。當使用者要求「定時、每天、每隔、提醒、排程、固定時間執行」之類任務時，\
         請直接在終端機執行下列指令完成設定——這是可用的內建功能，不要拒絕，也不要只用文字描述而不執行：\n\
         - 新增定時對話：`{bin} schedule add-turn --name \"<名稱>\" --cron \"<分 時 日 月 週>\" --scope \"{scope}\" --prompt \"<到時要執行的提示詞>\"`\n\
         - 列出所有排程：`{bin} schedule list`\n\
         - 立即觸發一次：`{bin} schedule trigger --id <id>`\n\
         - 停用／啟用／刪除：`{bin} schedule disable|enable|rm --id <id>`\n\
         規則：cron 為標準 5 欄位（分 時 日 月 週）。除非使用者另外指定 scope，否則一律沿用上面的 `--scope`。\
         建立完成後，用一句話回報排程名稱、cron 設定與用途即可。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_memory::MemoryKind;

    fn hit(scope: &str, text: &str) -> RecallHit {
        RecallHit {
            id: 1,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: text.to_string(),
            score: 1.0,
        }
    }

    #[test]
    fn build_prompt_includes_role_and_input() {
        let p = build_prompt(Role::Fixer, &[], "fix the bug");
        assert!(!p.contains("孫悟空"));
        assert!(p.contains("你是 Fixer"));
        assert!(p.contains("fix the bug"));
    }

    #[test]
    fn build_prompt_includes_memory_block_when_hits_present() {
        let hits = vec![hit("project:Wukong", "earlier decision")];
        let p = build_prompt(Role::Oracle, &hits, "what now?");
        assert!(p.contains("[相關記憶]"));
        assert!(p.contains("earlier decision"));
        assert!(p.contains("你是 Oracle"));
    }

    #[test]
    fn build_prompt_with_skill_includes_skill_block() {
        let skill = wukong_skills::find("test-driven-development").unwrap();
        let p = build_prompt_with_skill(Role::Fixer, Some(skill), &[], "fix the bug");
        assert!(p.contains("[技能規範指引]"));
        assert!(p.contains("test-driven-development"));
        assert!(
            p.contains("crates/wukong-skills/assets/superpowers/test-driven-development/SKILL.md")
        );
        assert!(p.contains("你是 Fixer"));
        assert!(p.contains("fix the bug"));
    }

    #[test]
    fn build_prompt_with_skill_omits_skill_block_when_absent() {
        let p = build_prompt_with_skill(Role::Oracle, None, &[], "think about it");
        assert!(!p.contains("[技能規範指引]"));
        assert!(p.contains("你是 Oracle"));
        assert!(p.contains("think about it"));
    }

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
        assert!(hint.contains("--scope \"project:Wukong\""));
        assert!(hint.contains("`wukong schedule list`"));
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
