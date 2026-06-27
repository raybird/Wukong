use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn resolve_skill_workspace() -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("WUKONG_WORKSPACE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    std::env::current_dir()
}

pub fn materialize_default_runtime_skills() -> io::Result<PathBuf> {
    let workspace = resolve_skill_workspace()?;
    materialize_runtime_skills(&workspace)
}

pub fn materialize_runtime_skills(workspace: &Path) -> io::Result<PathBuf> {
    let root = workspace.join(".wukong/skills/superpowers");
    let source = wukong_skills::source_content();
    let source_path = root.join("SOURCE.md");

    if fs::read_to_string(&source_path)
        .map(|existing| existing == source)
        .unwrap_or(false)
    {
        return Ok(root);
    }

    let parent = root.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "skill asset root has no parent")
    })?;
    let tmp = parent.join(format!(".superpowers.tmp.{}", std::process::id()));
    let old = parent.join(format!(".superpowers.old.{}", std::process::id()));

    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&old);
    fs::create_dir_all(&tmp)?;

    for skill in wukong_skills::all() {
        let skill_dir = tmp.join(skill.name);
        fs::create_dir_all(&skill_dir)?;
        fs::write(skill_dir.join("SKILL.md"), skill.content)?;
    }
    fs::write(tmp.join("SOURCE.md"), source)?;

    if root.exists() {
        fs::rename(&root, &old)?;
    }
    if let Err(err) = fs::rename(&tmp, &root) {
        if old.exists() {
            let _ = fs::rename(&old, &root);
        }
        let _ = fs::remove_dir_all(&tmp);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&old);

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_current_dir_when_env_missing() {
        let temp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::remove_var("WUKONG_WORKSPACE");
        std::env::set_current_dir(temp.path()).unwrap();

        let resolved = resolve_skill_workspace().unwrap();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn workspace_dir_uses_wukong_workspace_when_set() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", temp.path());

        let resolved = resolve_skill_workspace().unwrap();

        std::env::remove_var("WUKONG_WORKSPACE");
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn materialize_writes_skill_files_under_workspace() {
        let temp = tempfile::tempdir().unwrap();

        let root = materialize_runtime_skills(temp.path()).unwrap();

        assert_eq!(root, temp.path().join(".wukong/skills/superpowers"));
        assert!(root.join("brainstorming/SKILL.md").is_file());
        assert!(root.join("SOURCE.md").is_file());
    }
}
