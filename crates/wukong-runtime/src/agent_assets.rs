//! 2026-11-26（#45）：Wukong 管理的 OpenCode V2 agent 定義。
//!
//! primary agent `wukong` 帶人格（工作區的 SOUL.md）、派工規則與輸出要求；五個
//! subagent 沿用 [`Role`] 的描述與角色卡。定義寫在工作區的 `.opencode/agents/`：
//! 本機按需、常駐 server 與 CLI 三種模式都會讀到這裡，server 執行中改寫也不用重啟
//! （docs/issues/issue-0045/probe-agents.md）。不寫到全域設定目錄：binary 模式的
//! HOME 是使用者自己的家目錄，寫在那裡會改到使用者在 Wukong 以外使用的 OpenCode。
//!
//! 只寫這六個檔案，同一個目錄裡使用者自己的定義不動。內容沒變時不寫；有變時以
//! 同目錄的暫存檔改名替換，V2 的檔案監看不會讀到寫到一半的檔案。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use wukong_orchestrator::Role;

/// 定義檔所在的目錄（相對於工作區）。
pub const AGENTS_DIR: &str = ".opencode/agents";
/// primary agent 的名稱，也是回合要指定的 agent。
pub const PRIMARY_AGENT: &str = "wukong";

/// 工作區沒有 SOUL.md（或是空的）時用的人格，與映像、installer 複製的範本相同。
const DEFAULT_SOUL: &str = include_str!("../../../workspace/SOUL.md");
/// frontmatter 裡的 YAML 註解：不會進 prompt，只提醒手動修改會被覆寫。
const MANAGED_NOTE: &str = "# 由 Wukong 管理：啟動時依版本內容覆寫；要改人格請改工作區的 SOUL.md";

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 一個 agent 定義檔：`{name}.md` 與它的完整內容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub name: &'static str,
    pub content: String,
}

/// 什麼樣的任務派給這個角色，寫進 primary agent 的派工規則。
/// 措辭沿用 #45 Task 1.4 抽樣時的版本（big-pickle 實作題 3/3 派給 fixer）。
fn dispatch_rule(role: Role) -> &'static str {
    match role {
        Role::Explorer => "掃描專案結構、追蹤依賴",
        Role::Oracle => "架構取捨、重構方案、難解的 Bug",
        Role::Librarian => "撰寫文件、註解、翻譯",
        Role::Fixer => "建立或修改工作區裡的檔案、寫程式、補測試",
        Role::Designer => "介面結構、互動與視覺",
    }
}

fn frontmatter(description: &str, mode: &str) -> String {
    format!("---\n{MANAGED_NOTE}\ndescription: {description}\nmode: {mode}\n---\n")
}

/// 依人格內容產生六個定義：primary agent `wukong` 在前，接著是五個 subagent。
/// `memoria_hint` 為 [`crate::persona::memoria_hint_enabled`]：開啟時 primary agent 多一段長期記憶能力。
pub fn agent_definitions(soul: &str, memoria_hint: bool) -> Vec<AgentDefinition> {
    let mut rules = String::new();
    for role in Role::all() {
        rules.push_str(&format!(
            "- {}：派給 {}\n",
            dispatch_rule(role),
            role.name()
        ));
    }
    let primary = format!(
        "{}{}\n\n[派工]\n依任務性質決定自己回答，或用 subagent 工具派工：\n{rules}\
         簡單的問題直接回答，不要派工。\n\n{}\n\n{}\n",
        frontmatter(
            "孫悟空，主要助手，決定自己回答或派工給專門的 subagent",
            "primary"
        ),
        soul.trim(),
        crate::persona::final_answer_directive(),
        crate::persona::scheduling_capability_guide(&crate::persona::scheduling_bin()),
    );
    let primary = if memoria_hint {
        format!("{primary}\n{}\n", crate::persona::memoria_capability_hint())
    } else {
        primary
    };
    let mut defs = vec![AgentDefinition {
        name: PRIMARY_AGENT,
        content: primary,
    }];
    for role in Role::all() {
        defs.push(AgentDefinition {
            name: role.name(),
            content: format!(
                "{}{}\n\n完成後回報：你做了什麼、結果如何、有無注意事項。\n",
                frontmatter(role.description(), "subagent"),
                role.card(),
            ),
        });
    }
    defs
}

/// 工作區的 SOUL.md；沒有或是空的時用內建的範本。
fn workspace_soul(workspace: &Path) -> String {
    match fs::read_to_string(workspace.join("SOUL.md")) {
        Ok(text) if !text.trim().is_empty() => text,
        _ => DEFAULT_SOUL.to_string(),
    }
}

/// 把定義寫到 `workspace/.opencode/agents/`，回傳該目錄。長期記憶能力依
/// `WUKONG_MEMORIA_HINT`（[`crate::persona::memoria_hint_enabled`]）。
pub fn materialize_agent_definitions(workspace: &Path) -> io::Result<PathBuf> {
    write_definitions(workspace, crate::persona::memoria_hint_enabled())
}

/// 測試直接傳入開關：`WUKONG_MEMORIA_HINT` 是整個行程共用的，turn.rs 的測試會切換它。
fn write_definitions(workspace: &Path, memoria_hint: bool) -> io::Result<PathBuf> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| io::Error::other("agent definition write lock poisoned"))?;
    let dir = workspace.join(AGENTS_DIR);
    fs::create_dir_all(&dir)?;
    for def in agent_definitions(&workspace_soul(workspace), memoria_hint) {
        write_if_changed(&dir, &format!("{}.md", def.name), &def.content)?;
    }
    Ok(dir)
}

/// 寫到 backend 執行 opencode 的工作區（[`wukong_gateway::workspace_dir`]：
/// `WUKONG_WORKSPACE`，沒有設定時為 `~/.wukong/workspace`）。
pub fn materialize_default_agent_definitions() -> io::Result<PathBuf> {
    let workspace = wukong_gateway::workspace_dir()
        .ok_or_else(|| io::Error::other("no workspace directory"))?;
    materialize_agent_definitions(&workspace)
}

/// 把 `content` 寫到 `dir/file_name`：內容相同時不寫；不同時先寫同目錄的暫存檔再改名，
/// V2 的檔案監看不會讀到寫到一半的檔案。技能檔（[`crate::skill_assets`]）也用它。
pub(crate) fn write_if_changed(dir: &Path, file_name: &str, content: &str) -> io::Result<()> {
    let path = dir.join(file_name);
    if fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    // 暫存檔不以 .md 結尾，V2 不會把它當成定義或技能載入
    let tmp = dir.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        WRITE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    // #45 SCN-008：長期記憶能力只在 WUKONG_MEMORIA_HINT 開啟時寫進 primary agent 的定義。
    #[test]
    fn the_memoria_capability_follows_the_switch() {
        let on = &agent_definitions("soul", true)[0].content;
        let off = &agent_definitions("soul", false)[0].content;
        assert!(on.contains("[長期記憶能力]"), "{on}");
        assert!(!off.contains("[長期記憶能力]"), "{off}");
    }

    // #45 SCN-007：排程能力的固定說明在 primary agent 的定義裡；scope 由 session 指令層提供，
    // 定義裡只寫「目前的 scope」。
    #[test]
    fn the_primary_agent_explains_scheduling_without_a_scope() {
        let defs = agent_definitions("soul", false);
        let primary = &defs[0].content;
        assert!(primary.contains("[排程能力]"), "{primary}");
        assert!(primary.contains("schedule add-turn"), "{primary}");
        assert!(primary.contains("--scope \"<目前的 scope>\""), "{primary}");
        assert!(primary.contains("[目前的 scope]"), "{primary}");
        for sub in &defs[1..] {
            assert!(!sub.content.contains("[排程能力]"), "{}", sub.name);
        }
    }

    use super::*;

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(format!("{name}.md"))).unwrap()
    }

    #[test]
    fn first_write_creates_the_primary_agent_and_five_subagents() {
        let temp = tempfile::tempdir().unwrap();

        let dir = write_definitions(temp.path(), false).unwrap();

        assert_eq!(dir, temp.path().join(".opencode/agents"));
        let primary = read(&dir, "wukong");
        assert!(primary.contains("\nmode: primary\n"), "{primary}");
        assert!(primary.contains("孫悟空"), "{primary}");
        assert!(primary.contains("[輸出要求]"), "{primary}");
        for role in Role::all() {
            let sub = read(&dir, role.name());
            assert!(sub.contains("\nmode: subagent\n"), "{sub}");
            assert!(
                sub.contains(&format!("description: {}\n", role.description())),
                "{sub}"
            );
            assert!(sub.contains(role.card()), "{sub}");
            assert!(
                primary.contains(&format!("派給 {}\n", role.name())),
                "{primary}"
            );
        }
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "designer.md",
                "explorer.md",
                "fixer.md",
                "librarian.md",
                "oracle.md",
                "wukong.md"
            ],
            "only the six definitions, no leftover temp files"
        );
    }

    #[test]
    fn unchanged_content_is_not_rewritten() {
        let temp = tempfile::tempdir().unwrap();
        let dir = write_definitions(temp.path(), false).unwrap();
        let before = fs::metadata(dir.join("wukong.md"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        write_definitions(temp.path(), false).unwrap();

        let after = fs::metadata(dir.join("wukong.md"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn an_older_or_edited_definition_is_replaced_with_the_current_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".opencode/agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("fixer.md"), "---\nmode: subagent\n---\nold\n").unwrap();

        write_definitions(temp.path(), false).unwrap();

        assert!(read(&dir, "fixer").contains(Role::Fixer.card()));
    }

    #[test]
    fn other_definitions_in_the_directory_are_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".opencode/agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("mine.md"), "user agent\n").unwrap();

        write_definitions(temp.path(), false).unwrap();

        assert_eq!(read(&dir, "mine"), "user agent\n");
    }

    #[test]
    fn the_persona_comes_from_the_workspace_soul_and_falls_back_to_the_template() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("SOUL.md"), "你是測試用的人格。\n").unwrap();
        let dir = write_definitions(temp.path(), false).unwrap();
        assert!(read(&dir, "wukong").contains("你是測試用的人格。"));

        let bare = tempfile::tempdir().unwrap();
        let dir = write_definitions(bare.path(), false).unwrap();
        assert!(read(&dir, "wukong").contains(DEFAULT_SOUL.trim()));
    }

    /// 外迴圈用：把 agent 定義與技能寫到 `WUKONG_AGENT_PROBE_DIR`、遷移那裡的 AGENTS.md，再交給
    /// docs/issues/issue-0045/scripts/ 的 probe-managed-agents.sh、probe-managed-skills.sh
    /// 對真實 V2 檢查。
    #[test]
    #[ignore = "writes into WUKONG_AGENT_PROBE_DIR for the V2 probe"]
    fn write_definitions_for_probe() {
        let dir = std::env::var("WUKONG_AGENT_PROBE_DIR").expect("WUKONG_AGENT_PROBE_DIR");
        materialize_agent_definitions(Path::new(&dir)).unwrap();
        crate::skill_assets::materialize_skills(Path::new(&dir)).unwrap();
        let outcome = crate::workspace_template::migrate_agents_md(Path::new(&dir)).unwrap();
        println!("agents-md: {outcome:?}");
    }

    #[test]
    fn descriptions_are_single_line_plain_yaml() {
        // frontmatter 的 description 不加引號：不能換行，也不能含 ASCII 的「: 」或開頭的特殊字元
        for def in agent_definitions(DEFAULT_SOUL, false) {
            let line = def
                .content
                .lines()
                .find(|l| l.starts_with("description: "))
                .unwrap();
            let value = line.trim_start_matches("description: ");
            assert!(!value.contains(": ") && !value.contains(" #"), "{value}");
            assert!(
                !value.starts_with(['"', '\'', '[', '{', '&', '*', '!', '|', '>', '%', '@', '`'])
            );
        }
    }
}
