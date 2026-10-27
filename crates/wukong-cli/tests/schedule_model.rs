//! issue-0026：排程專屬模型的 CLI 黑箱測試，執行真正的 binary。
//!
//! 資料庫用 `WUKONG_MEMORY_DB` 指定：`--db` 放在子命令前會把子命令當成 prompt
//! 跑一個真的回合（project.md 地雷 W-5）。

use std::path::Path;

fn wukong(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env(
            "WUKONG_MEMORY_DB",
            format!("sqlite://{}", dir.join("memory.db").display()),
        )
        .env("WUKONG_SETTINGS_FILE", dir.join("settings.json"))
        .env_remove("WUKONG_AGENT_SERVER_URL")
        .env_remove("WUKONG_EMBED")
        .env_remove("WUKONG_MD_DIR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "wukong {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn created_id(stdout: &str) -> String {
    stdout
        .trim()
        .strip_prefix("已建立排程: ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("unexpected add output: {stdout}"))
        .to_string()
}

fn listed_kind(dir: &Path, id: &str) -> String {
    let list = wukong(dir, &["schedule", "list"]);
    let line = list
        .lines()
        .find(|line| line.starts_with(id))
        .unwrap_or_else(|| panic!("{id} not listed: {list}"));
    line.split('\t')
        .find_map(|field| field.strip_prefix("kind="))
        .unwrap()
        .to_string()
}

// SCN-003、SCN-004
#[test]
fn schedule_model_can_be_set_listed_and_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let id = created_id(&wukong(
        dir,
        &[
            "schedule",
            "add-turn",
            "--name",
            "daily",
            "--cron",
            "0 9 * * *",
            "--scope",
            "project:T",
            "--prompt",
            "review",
            "--model",
            "nvidia/x/y",
        ],
    ));
    assert_eq!(
        listed_kind(dir, &id),
        "turn(scope=project:T,model=nvidia/x/y)"
    );

    assert_eq!(
        wukong(dir, &["schedule", "set-model", "--id", &id]).trim(),
        "已改回使用預設模型"
    );
    assert_eq!(
        listed_kind(dir, &id),
        "turn(scope=project:T,model=使用預設)"
    );

    assert_eq!(
        wukong(
            dir,
            &[
                "schedule",
                "set-model",
                "--id",
                &id,
                "--model",
                "opencode/a"
            ]
        )
        .trim(),
        "已設定排程模型：opencode/a"
    );
    assert_eq!(
        listed_kind(dir, &id),
        "turn(scope=project:T,model=opencode/a)"
    );
}

// SCN-003：不存在的 id 與維護排程不能設定模型。
#[test]
fn schedule_set_model_reports_unknown_and_maintenance_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    assert_eq!(
        wukong(
            dir,
            &[
                "schedule",
                "set-model",
                "--id",
                "missing",
                "--model",
                "opencode/a"
            ]
        )
        .trim(),
        "找不到排程"
    );

    let id = created_id(&wukong(
        dir,
        &[
            "schedule",
            "add-maintenance",
            "--name",
            "snap",
            "--cron",
            "0 2 * * *",
            "--task",
            "snapshot",
        ],
    ));
    assert_eq!(
        wukong(
            dir,
            &[
                "schedule",
                "set-model",
                "--id",
                &id,
                "--model",
                "opencode/a"
            ]
        )
        .trim(),
        "維護排程不使用模型，無法設定"
    );
}
