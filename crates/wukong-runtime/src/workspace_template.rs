//! 2026-11-26（#45 SCN-009）：既有部署的工作區 AGENTS.md 遷移到新版範本。
//!
//! 人格改由 Wukong 管理的 agent 定義提供（[`crate::agent_assets`]），技能改由 V2 的
//! skill 機制提供（[`crate::skill_assets`]），所以新版範本拿掉了人格那一句與「技能動態
//! 加載機制」段落。舊範本會被複製進工作區後就不再更新，這裡在入口啟動時處理：
//!
//! - 與某一版舊範本完全相同：先備份成 `AGENTS.md.bak-{版本}`，再換成新版；
//! - 改過的：不動，回報其中過時的段落，由入口寫進 log；
//! - 已是新版或不存在：不動（不存在時由 Docker entrypoint 或 installer 建立）。
//!
//! 舊範本原文放在 `assets/retired-agents-md/`，取自 `git log -- workspace/AGENTS.md`
//! 的每一版。之後再改範本時，把改之前的版本加進 [`RETIRED`]。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::agent_assets::write_if_changed;

pub const AGENTS_FILE: &str = "AGENTS.md";
const CURRENT: &str = include_str!("../../../workspace/AGENTS.md");

/// 歷代範本：提交的短 SHA 與當時的內容。
const RETIRED: &[(&str, &str)] = &[
    (
        "ac60ebe",
        include_str!("../assets/retired-agents-md/ac60ebe.md"),
    ),
    (
        "e72cccd",
        include_str!("../assets/retired-agents-md/e72cccd.md"),
    ),
    (
        "65511d2",
        include_str!("../assets/retired-agents-md/65511d2.md"),
    ),
];

/// 改過的 AGENTS.md 裡出現這些字樣時，回報對應的段落已過時。
const OUTDATED: &[(&str, &str)] = &[
    (
        "[技能規範指引]",
        "「技能動態加載機制」：prompt 已不再附技能規範指引，技能改由 OpenCode 的 skill 機制提供",
    ),
    (
        ".wukong/skills",
        "技能路徑 .wukong/skills：技能已改放在 .opencode/skills，由 OpenCode 自動載入",
    ),
];

#[derive(Debug, PartialEq, Eq)]
pub enum AgentsMdMigration {
    /// 工作區沒有 AGENTS.md。
    Missing,
    /// 已是新版範本。
    Current,
    /// 與舊範本 `from` 相同，已備份到 `backup` 並換成新版。
    Replaced { from: &'static str, backup: PathBuf },
    /// 使用者改過，沒有動；`outdated` 是其中過時的段落說明（可能是空的）。
    Customized { outdated: Vec<&'static str> },
}

/// 依上面的規則處理 `workspace/AGENTS.md`。
pub fn migrate_agents_md(workspace: &Path) -> io::Result<AgentsMdMigration> {
    let path = workspace.join(AGENTS_FILE);
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(AgentsMdMigration::Missing),
        Err(err) => return Err(err),
    };
    if existing == CURRENT {
        return Ok(AgentsMdMigration::Current);
    }
    if let Some((from, _)) = RETIRED.iter().find(|(_, text)| *text == existing) {
        let backup = workspace.join(format!("{AGENTS_FILE}.bak-{from}"));
        fs::write(&backup, &existing)?;
        write_if_changed(workspace, AGENTS_FILE, CURRENT)?;
        return Ok(AgentsMdMigration::Replaced { from, backup });
    }
    let outdated = OUTDATED
        .iter()
        .filter(|(marker, _)| existing.contains(marker))
        .map(|(_, note)| *note)
        .collect();
    Ok(AgentsMdMigration::Customized { outdated })
}

/// 對 backend 的工作區（[`wukong_gateway::workspace_dir`]）執行遷移。
pub fn migrate_default_agents_md() -> io::Result<(PathBuf, AgentsMdMigration)> {
    let workspace = wukong_gateway::workspace_dir()
        .ok_or_else(|| io::Error::other("no workspace directory"))?;
    let outcome = migrate_agents_md(&workspace)?;
    Ok((workspace.join(AGENTS_FILE), outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_with(text: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(AGENTS_FILE), text).unwrap();
        temp
    }

    fn read(temp: &tempfile::TempDir, name: &str) -> String {
        fs::read_to_string(temp.path().join(name)).unwrap()
    }

    #[test]
    fn the_new_template_has_no_persona_or_skill_loading_section() {
        assert!(!CURRENT.contains("孫悟空"));
        for (marker, _) in OUTDATED {
            assert!(!CURRENT.contains(marker), "{marker}");
        }
        assert!(!CURRENT.contains("技能動態加載"));
        assert!(!CURRENT.contains("SKILL.md"));
    }

    #[test]
    fn every_retired_template_is_backed_up_and_replaced() {
        for (sha, text) in RETIRED {
            let temp = workspace_with(text);

            let outcome = migrate_agents_md(temp.path()).unwrap();

            let backup = temp.path().join(format!("AGENTS.md.bak-{sha}"));
            assert_eq!(
                outcome,
                AgentsMdMigration::Replaced {
                    from: sha,
                    backup: backup.clone()
                }
            );
            assert_eq!(fs::read_to_string(&backup).unwrap(), *text);
            assert_eq!(read(&temp, AGENTS_FILE), CURRENT);
            // 再啟動一次就是新版，不再動它
            assert_eq!(
                migrate_agents_md(temp.path()).unwrap(),
                AgentsMdMigration::Current
            );
        }
    }

    #[test]
    fn a_customized_file_is_left_alone_and_its_outdated_sections_are_reported() {
        let (_, ac60ebe) = RETIRED[0];
        let edited = format!("{ac60ebe}\n## 我自己的規則\n- 回答一律附上出處\n");
        let temp = workspace_with(&edited);

        let outcome = migrate_agents_md(temp.path()).unwrap();

        assert_eq!(read(&temp, AGENTS_FILE), edited);
        let AgentsMdMigration::Customized { outdated } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(outdated.len(), 1, "{outdated:?}");
        assert!(outdated[0].contains("技能動態加載機制"));
        assert!(fs::read_dir(temp.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".bak-")));
    }

    #[test]
    fn a_customized_file_without_outdated_sections_reports_nothing() {
        let temp = workspace_with("# 我的專案規則\n");

        assert_eq!(
            migrate_agents_md(temp.path()).unwrap(),
            AgentsMdMigration::Customized { outdated: vec![] }
        );
    }

    #[test]
    fn missing_and_current_files_are_not_touched() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            migrate_agents_md(empty.path()).unwrap(),
            AgentsMdMigration::Missing
        );
        assert!(!empty.path().join(AGENTS_FILE).exists());

        let current = workspace_with(CURRENT);
        assert_eq!(
            migrate_agents_md(current.path()).unwrap(),
            AgentsMdMigration::Current
        );
    }
}
