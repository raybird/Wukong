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
use std::time::Duration;

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
/// 這個行程改寫過 primary agent 的定義、還沒確認 backend 用上時，記著它的內文（[`system_of`]）。
static PENDING_PRIMARY_SYSTEM: Mutex<Option<String>> = Mutex::new(None);
/// 等 backend 用上改寫後的定義最多等多久；逾時照常送出（實測幾百毫秒內生效）。
const PRIMARY_LOAD_TIMEOUT: Duration = Duration::from_secs(5);

/// 一個 agent 定義檔：`{name}.md` 與它的完整內容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub name: &'static str,
    pub content: String,
}

/// 什麼樣的任務派給這個角色，寫進 primary agent 的派工規則。
/// 2026-11-26（#45 Task 6.1）：Task 1.4 的措辭在 nemotron 上實作題 0/3、探索 1/3、寫文件
/// 0/3 派工，primary 都自己讀檔改檔；改成指明「不要自己逐檔讀」「文件交給 librarian」。
/// 第三輪（primary 已不能改檔）新建 README 有 3 次交給 fixer：寫明新建文件檔也算文件，
/// fixer 只管文件以外的檔案。
fn dispatch_rule(role: Role) -> &'static str {
    match role {
        Role::Explorer => "掃描專案結構、整理模組之間的關係或依賴（不要自己逐一讀檔整理）",
        Role::Oracle => "架構取捨、重構方案、難解的 Bug",
        Role::Librarian => {
            "撰寫或更新文件（README、說明文件、*.md）、註解、翻譯；新建文件檔也派給 librarian，不交給 fixer"
        }
        Role::Fixer => {
            "文件以外的檔案與執行指令：寫程式、補測試、跑測試、改設定、產出要回傳的檔案"
        }
        Role::Designer => "介面結構、互動與視覺的設計建議",
    }
}

/// primary agent 不能改檔，也只能執行排程與 memoria 指令（#45 Task 6.1，使用者 2026-11-26 決定）：
/// 改檔、跑測試與其他指令都要派工。
/// - `edit`：V2 套用到所有改檔工具；
/// - `execute`：跑程式碼的工具；
/// - `shell`：資源是整段指令。只關 `edit` 時，primary 改用 heredoc 寫檔（dispatch-eval.md 第二輪）。
///
/// V2 依序比對、後面的規則優先，所以先拒絕全部再逐項允許。`bin` 是排程說明裡寫的指令
/// （[`crate::persona::scheduling_bin`]）；memoria 指令只在長期記憶能力開啟時允許。
fn primary_permission(bin: &str, memoria_hint: bool) -> String {
    let mut allowed = vec![format!("{bin} schedule *")];
    if memoria_hint {
        allowed.push("memoria *".to_string());
        allowed.push("memoria-vector-sync*".to_string());
    }
    let mut out =
        String::from("permission:\n  edit: deny\n  execute: deny\n  shell:\n    \"*\": deny\n");
    for pattern in allowed {
        let quoted = pattern.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("    \"{quoted}\": allow\n"));
    }
    out
}

fn frontmatter(description: &str, mode: &str, extra: &str) -> String {
    format!("---\n{MANAGED_NOTE}\ndescription: {description}\nmode: {mode}\n{extra}---\n")
}

/// 依人格內容產生六個定義：primary agent `wukong` 在前，接著是五個 subagent。
/// `memoria_hint` 為 [`crate::persona::memoria_hint_enabled`]：開啟時 primary agent 多一段長期記憶能力。
pub fn agent_definitions(soul: &str, memoria_hint: bool) -> Vec<AgentDefinition> {
    let bin = crate::persona::scheduling_bin();
    let mut rules = String::new();
    for role in Role::all() {
        rules.push_str(&format!(
            "- {}：派給 {}\n",
            dispatch_rule(role),
            role.name()
        ));
    }
    let primary = format!(
        "{}{}\n\n[派工]\n你不能自己建立或修改檔案，終端機也只能執行這份說明列出的排程與長期記憶指令（權限已關閉）；\
         需要改檔、跑測試或執行其他指令的工作一律派工。依任務性質決定自己回答，\
         或用 subagent 工具派工：\n{rules}\
         簡單的問題直接回答，不要派工。派工時把使用者提到的路徑、附件位置與訊息裡的\
         `[產出目錄]` 一併寫進交代內容，subagent 看不到使用者的原始訊息。\
         subagent 在背景執行，完成時會回報給你：派工後就結束這次回覆、等它回報，\
         不要在等待時反覆讀檔或查詢進度。\n\n{}\n\n{}\n\n{}\n",
        frontmatter(
            "孫悟空，主要助手，決定自己回答或派工給專門的 subagent",
            "primary",
            &primary_permission(&bin, memoria_hint),
        ),
        soul.trim(),
        crate::persona::final_answer_directive(),
        crate::persona::scheduling_capability_guide(&bin),
        crate::persona::file_interaction_rules(),
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
                "{}{}\n\n{}\n\n完成後回報：你做了什麼、結果如何、有無注意事項。\n",
                frontmatter(role.description(), "subagent", ""),
                role.card(),
                crate::persona::file_interaction_rules(),
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
    let (dir, rewritten) = write_definitions(workspace, crate::persona::memoria_hint_enabled())?;
    if let Some(system) = rewritten {
        if let Ok(mut pending) = PENDING_PRIMARY_SYSTEM.lock() {
            *pending = Some(system);
        }
    }
    Ok(dir)
}

/// 取出還沒確認的 primary agent 內文；取出後清掉，只有下一個回合會等。
pub(crate) fn take_pending_primary_system() -> Option<String> {
    PENDING_PRIMARY_SYSTEM.lock().ok()?.take()
}

/// 2026-11-26（#45 SCN-008）：定義剛改寫過時，送 prompt 前等 backend 用上新內容。常駐 V2
/// server 靠檔案監看重新載入，入口啟動時改寫、緊接著送出的回合（CLI 單次執行、重啟時積壓的
/// Telegram 訊息或到期的排程）會用到舊定義。其他 backend 不用等，立刻回來。
pub(crate) async fn wait_until_primary_loaded(
    backend: &impl wukong_gateway::backend::AiBackend,
    pending: Option<String>,
) {
    let Some(system) = pending else {
        return;
    };
    if !backend
        .wait_for_agent_system(PRIMARY_AGENT, &system, PRIMARY_LOAD_TIMEOUT)
        .await
    {
        eprintln!(
            "warning: agent_definition_not_loaded agent={PRIMARY_AGENT} waited={}s; sending anyway",
            PRIMARY_LOAD_TIMEOUT.as_secs()
        );
    }
}

/// 測試直接傳入開關：`WUKONG_MEMORIA_HINT` 是整個行程共用的，turn.rs 的測試會切換它。
/// 回傳目錄，以及 primary agent 的定義有改寫時它的內文。
fn write_definitions(
    workspace: &Path,
    memoria_hint: bool,
) -> io::Result<(PathBuf, Option<String>)> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| io::Error::other("agent definition write lock poisoned"))?;
    let dir = workspace.join(AGENTS_DIR);
    fs::create_dir_all(&dir)?;
    let mut rewritten = None;
    for def in agent_definitions(&workspace_soul(workspace), memoria_hint) {
        if write_if_changed(&dir, &format!("{}.md", def.name), &def.content)?
            && def.name == PRIMARY_AGENT
        {
            rewritten = Some(system_of(&def.content).to_string());
        }
    }
    Ok((dir, rewritten))
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
/// 回傳有沒有寫入。
pub(crate) fn write_if_changed(dir: &Path, file_name: &str, content: &str) -> io::Result<bool> {
    let path = dir.join(file_name);
    if fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(false);
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
    })?;
    Ok(true)
}

/// 定義檔 frontmatter 之後的內文，去掉頭尾空白：V2 的 `/api/agent` 以 `system` 回傳的就是這段。
fn system_of(content: &str) -> &str {
    content
        .split_once("\n---\n")
        .map_or(content, |(_, body)| body)
        .trim()
}

#[cfg(test)]
mod tests {
    // #45 SCN-006：檔案互動規則在 primary agent 的定義裡，每回合只附產出目錄。
    #[test]
    fn the_primary_agent_carries_the_file_rules() {
        let primary = &agent_definitions("soul", false)[0].content;
        assert!(primary.contains("[檔案互動規則]"), "{primary}");
        assert!(primary.contains("`[產出目錄] <路徑>`"), "{primary}");
    }

    // #45 SCN-008：改寫 primary agent 的定義時回報內文，供下一個回合確認 backend 已用上。
    #[test]
    fn rewriting_the_primary_definition_reports_its_system_text() {
        let temp = tempfile::tempdir().unwrap();
        let (_, first) = write_definitions(temp.path(), false).unwrap();
        let first = first.expect("the first write rewrites the primary definition");
        assert_eq!(
            first,
            system_of(&agent_definitions(DEFAULT_SOUL, false)[0].content)
        );
        assert!(!first.contains("mode: primary"), "{first}");

        let (_, again) = write_definitions(temp.path(), false).unwrap();
        assert_eq!(again, None);

        let (_, switched) = write_definitions(temp.path(), true).unwrap();
        assert!(switched.unwrap().contains("[長期記憶能力]"));
    }

    // 2026-11-26 實測：定義檔是下面這段時，V2 `/api/agent` 回的 system 是 "第一行。\n\n第二段 ALPHA。"
    #[test]
    fn the_system_text_is_what_v2_lists_for_the_definition() {
        let file =
            "---\ndescription: shape probe\nmode: primary\n---\n第一行。\n\n第二段 ALPHA。\n";
        assert_eq!(system_of(file), "第一行。\n\n第二段 ALPHA。");
    }

    struct WaitRecorder(std::sync::Mutex<Vec<(String, String, Duration)>>);
    impl wukong_gateway::backend::AiBackend for WaitRecorder {
        async fn run(
            &self,
            _req: wukong_gateway::backend::AgentRequest,
        ) -> Result<wukong_gateway::backend::AgentResponse, wukong_gateway::error::GatewayError>
        {
            unreachable!("waiting never sends a prompt")
        }
        async fn wait_for_agent_system(
            &self,
            agent: &str,
            system: &str,
            timeout: Duration,
        ) -> bool {
            self.0
                .lock()
                .unwrap()
                .push((agent.to_string(), system.to_string(), timeout));
            true
        }
    }

    #[tokio::test]
    async fn a_pending_definition_is_waited_for_and_none_is_not() {
        let backend = WaitRecorder(std::sync::Mutex::new(Vec::new()));
        wait_until_primary_loaded(&backend, None).await;
        assert!(backend.0.lock().unwrap().is_empty());

        wait_until_primary_loaded(&backend, Some("新的內文".to_string())).await;
        assert_eq!(
            *backend.0.lock().unwrap(),
            vec![(
                "wukong".to_string(),
                "新的內文".to_string(),
                PRIMARY_LOAD_TIMEOUT
            )]
        );
    }

    // #45 SCN-008：長期記憶能力只在 WUKONG_MEMORIA_HINT 開啟時寫進 primary agent 的定義。
    #[test]
    fn the_memoria_capability_follows_the_switch() {
        let on = &agent_definitions("soul", true)[0].content;
        let off = &agent_definitions("soul", false)[0].content;
        assert!(on.contains("[長期記憶能力]"), "{on}");
        assert!(!off.contains("[長期記憶能力]"), "{off}");
    }

    #[test]
    fn only_the_primary_agent_is_denied_file_edits() {
        // #45 Task 6.1：nemotron 的 primary 實作題 0/3 派工，都是自己改檔（dispatch-eval.md）
        let defs = agent_definitions("soul", false);
        let frontmatter_of = |content: &str| content.split_once("\n---\n").unwrap().0.to_string();
        let primary = &defs[0].content;
        assert!(
            frontmatter_of(primary).ends_with(&format!(
                "\n{}",
                primary_permission(&crate::persona::scheduling_bin(), false).trim_end()
            )),
            "{primary}"
        );
        assert!(
            system_of(primary).contains("不能自己建立或修改檔案"),
            "{primary}"
        );
        assert!(system_of(primary).contains("[產出目錄]"), "{primary}");
        // Task 6.2：subagent 在背景執行，primary 派工後反覆 read 檢查檔案，tool 任務多出 3～8 步
        assert!(
            system_of(primary).contains("派工後就結束這次回覆"),
            "{primary}"
        );
        for sub in &defs[1..] {
            assert!(
                !frontmatter_of(&sub.content).contains("permission"),
                "{}",
                sub.name
            );
            assert!(sub.content.contains("[檔案互動規則]"), "{}", sub.name);
        }
    }

    // 第二輪 primary 改用 shell 的 heredoc 與 execute 寫檔（dispatch-eval.md）：shell 只留排程與
    // memoria 指令，其他都拒絕；規則依序比對、後面的優先，所以先拒絕全部再逐項允許。
    #[test]
    fn the_primary_shell_only_allows_scheduling_and_memoria() {
        let off = primary_permission("/opt/my \"bin\"/wukong", false);
        assert_eq!(
            off,
            "permission:\n  edit: deny\n  execute: deny\n  shell:\n    \"*\": deny\n    \
             \"/opt/my \\\"bin\\\"/wukong schedule *\": allow\n"
        );
        let on = primary_permission("wukong", true);
        assert!(on.ends_with(
            "    \"wukong schedule *\": allow\n    \"memoria *\": allow\n    \
             \"memoria-vector-sync*\": allow\n"
        ));
        assert!(agent_definitions("soul", true)[0]
            .content
            .contains("\"memoria *\": allow"));
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

        let (dir, _) = write_definitions(temp.path(), false).unwrap();

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
        let (dir, _) = write_definitions(temp.path(), false).unwrap();
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
        let (dir, _) = write_definitions(temp.path(), false).unwrap();
        assert!(read(&dir, "wukong").contains("你是測試用的人格。"));

        let bare = tempfile::tempdir().unwrap();
        let (dir, _) = write_definitions(bare.path(), false).unwrap();
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
