//! 2026-11-26（#45）：內嵌的 Superpowers 技能寫到工作區的 `.opencode/skills/`。
//!
//! V2 會從這裡讀技能（`/api/skill` 列得到），模型依 description 自己用 `skill`
//! 工具載入，prompt 不必再附技能規範指引或 SKILL.md 路徑
//! （docs/issues/issue-0045/probe-skills.md）。原本寫的 `.wukong/skills/superpowers`
//! 不在 V2 的探索範圍裡，已不再寫入。
//!
//! 每個技能一個 `{name}/SKILL.md`，內容沒變時不寫；同目錄裡使用者自己的技能不動。
//! 上游出處只留在 repo 的 `crates/wukong-skills/assets/superpowers/SOURCE.md`：V2 會把
//! `.opencode/skills/` 底下的任何 .md 檔當成技能（2026-11-26 實測 `SUPERPOWERS-SOURCE.md`
//! 被列成一個技能），所以不寫進工作區。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::agent_assets::write_if_changed;

/// 技能所在的目錄（相對於工作區），和 agent 定義同在 `.opencode/` 底下。
pub const SKILLS_DIR: &str = ".opencode/skills";

/// 把內嵌的技能寫到 `workspace/.opencode/skills/`，回傳該目錄。
pub fn materialize_skills(workspace: &Path) -> io::Result<PathBuf> {
    let root = workspace.join(SKILLS_DIR);
    for skill in wukong_skills::all() {
        let dir = root.join(skill.name);
        fs::create_dir_all(&dir)?;
        write_if_changed(&dir, "SKILL.md", skill.content)?;
    }
    Ok(root)
}

/// 寫到 backend 執行 opencode 的工作區（[`wukong_gateway::workspace_dir`]）。
pub fn materialize_default_skills() -> io::Result<PathBuf> {
    let workspace = wukong_gateway::workspace_dir()
        .ok_or_else(|| io::Error::other("no workspace directory"))?;
    materialize_skills(&workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_every_embedded_skill_under_opencode_skills() {
        let temp = tempfile::tempdir().unwrap();

        let root = materialize_skills(temp.path()).unwrap();

        assert_eq!(root, temp.path().join(".opencode/skills"));
        for skill in wukong_skills::all() {
            let text = fs::read_to_string(root.join(skill.name).join("SKILL.md")).unwrap();
            assert_eq!(text, skill.content);
            assert!(
                text.starts_with(&format!("---\nname: {}\n", skill.name)),
                "{}",
                skill.name
            );
        }
        // 只有技能的子目錄：V2 會把根目錄的 .md 檔也當成技能
        let entries: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| !e.file_type().unwrap().is_dir())
            .map(|e| e.file_name())
            .collect();
        assert!(entries.is_empty(), "{entries:?}");
        assert!(!temp.path().join(".wukong").exists());
    }

    #[test]
    fn unchanged_skills_are_not_rewritten_and_edited_ones_are_restored() {
        let temp = tempfile::tempdir().unwrap();
        let root = materialize_skills(temp.path()).unwrap();
        let tdd = root.join("test-driven-development/SKILL.md");
        let brainstorm = root.join("brainstorming/SKILL.md");
        let before = fs::metadata(&tdd).unwrap().modified().unwrap();
        fs::write(&brainstorm, "edited\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        materialize_skills(temp.path()).unwrap();

        assert_eq!(fs::metadata(&tdd).unwrap().modified().unwrap(), before);
        assert_eq!(
            fs::read_to_string(&brainstorm).unwrap(),
            wukong_skills::all()
                .iter()
                .find(|skill| skill.name == "brainstorming")
                .unwrap()
                .content
        );
    }

    #[test]
    fn other_skills_in_the_directory_are_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        let mine = temp.path().join(".opencode/skills/mine");
        fs::create_dir_all(&mine).unwrap();
        fs::write(mine.join("SKILL.md"), "user skill\n").unwrap();

        materialize_skills(temp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(mine.join("SKILL.md")).unwrap(),
            "user skill\n"
        );
    }

    #[test]
    fn concurrent_writers_leave_complete_files() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().to_path_buf();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let workspace = workspace.clone();
                std::thread::spawn(move || materialize_skills(&workspace))
            })
            .collect();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let root = temp.path().join(SKILLS_DIR);
        for skill in wukong_skills::all() {
            let text = fs::read_to_string(root.join(skill.name).join("SKILL.md")).unwrap();
            assert_eq!(text, skill.content);
            let leftovers: Vec<_> = fs::read_dir(root.join(skill.name))
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .filter(|n| n != "SKILL.md")
                .collect();
            assert!(leftovers.is_empty(), "{leftovers:?}");
        }
    }
}
