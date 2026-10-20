use std::time::Instant;
use wukong_gateway::backend::AgentCliBackend;
use wukong_orchestrator::plan_skill_chain_with_preferences;
use wukong_skills::route_options;

// issue-0011 步驟 1／4／5：以真實模型取 planner 的計畫分布與真實回合，結果記在
// docs/issues/issue-0011/implementation-plan.md。沒設 WUKONG_PLANNER_PROBE_MODEL 就失敗，
// 避免 `cargo test -- --ignored` 順手呼叫真實模型。實作計畫用的條件（空的 config 目錄，
// 不讓本機的 opencode 設定、全域 AGENTS.md 與 plugin 混進來；一次只跑一支）：
//   XDG_CONFIG_HOME=<空目錄> WUKONG_PLANNER_PROBE_MODEL=opencode/big-pickle \
//     cargo test -p wukong-runtime --test planner_probe <測試名> -- --ignored --nocapture
// WUKONG_PLANNER_PROBE_BASELINE=1 不帶上一回合，重現修改前 planner 看到的輸入。
// 注意：real_followup_turns 的 agent 有完整工具權限，可能離開臨時目錄在本機執行指令
// （曾因此建出 ~/.wukong/memory.db）；它也會改整個 process 的 WUKONG_WORKSPACE。
// A-tg 是 Telegram 實際送進 run_turn 的樣子：附加檔案互動規則
// （文字取自 wukong-telegram dispatch.rs 的 prompt_with_artifact_instruction）。
const TG_RULES: &str = "\n\n[Wukong 檔案互動規則]\n上傳附件已是可修改的工作副本，不要修改 .wukong/uploads 內的原始檔。若使用者要求建立、轉換、修改或回傳檔案，請將每個最終成品直接寫入此目錄：/workspace/.wukong/artifacts/user_tg-915354960/1297。只把要回傳給使用者的成品放入該目錄。";

// 格式同 run_turn 交給 planner 的上一回合（User／Assistant 兩筆記憶）；內容是
// real_followup_turns 先跑的 FACTS_TURN 與一次實際回覆，兩支探針看到同一段上下文。
const PREVIOUS_TURN: &str = "User: 先記住目前狀態，不需要查證：排程 run #869 開始 20 秒後失敗，錯誤是 session lease unavailable for scope user:tg-915354960。升級 v0.22.1 後的 9 次正式執行中，只有 #863（1506 秒）和 #870（796 秒）成功，另外 6 次是 OpenCode 逾時。請用一句話確認你記住了。\nAssistant: 我記住了：排程 run #869 開始 20 秒後因 `user:tg-915354960` 的 session lease unavailable 而失敗；升級 v0.22.1 後 9 次正式執行僅 #863（1506 秒）與 #870（796 秒）成功，另有 6 次因 OpenCode 逾時失敗。";

fn probe_model() -> String {
    std::env::var("WUKONG_PLANNER_PROBE_MODEL")
        .expect("set WUKONG_PLANNER_PROBE_MODEL (e.g. opencode/big-pickle) to call a real model")
}

const CASES: [(&str, &str); 3] = [
    ("A", "所以沒辦法跑成功嗎"),
    ("B", "幫我重新查一下現在排程的狀態"),
    ("C", "修好 scheduler 的逾時問題，並跑測試驗證"),
];

#[tokio::test]
#[ignore = "calls a real model through OpenCode"]
async fn planner_plan_distribution() {
    let model = probe_model();
    let samples: usize = std::env::var("WUKONG_PLANNER_PROBE_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let workspace = tempfile::tempdir().unwrap();
    // managed 模式會丟掉 --model，用 CLI backend 才能指定模型。
    let backend = AgentCliBackend {
        command: vec![
            "opencode".into(),
            "run".into(),
            "--model".into(),
            model.clone(),
        ],
        workspace: Some(workspace.path().to_path_buf()),
    };
    let baseline = std::env::var("WUKONG_PLANNER_PROBE_BASELINE").is_ok_and(|v| v == "1");
    let previous_turn = (!baseline).then_some(PREVIOUS_TURN);
    println!("model={model} samples={samples} baseline={baseline}");
    let tg_input = format!("{}{TG_RULES}", CASES[0].1);
    let cases = CASES
        .iter()
        .map(|(case, input)| (case.to_string(), input.to_string()))
        .chain([("A-tg".to_string(), tg_input)]);
    for (case, input) in cases {
        for sample in 1..=samples {
            let started = Instant::now();
            let steps = plan_skill_chain_with_preferences(
                &backend,
                &input,
                &route_options(),
                None,
                previous_turn,
            )
            .await
            .unwrap();
            let plan = steps
                .iter()
                .map(|s| {
                    format!(
                        "{}|{}",
                        s.role.name(),
                        s.skill_name.as_deref().unwrap_or("none")
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "case={case} sample={sample} steps={} plan={plan} secs={:.1}",
                steps.len(),
                started.elapsed().as_secs_f64()
            );
        }
    }
}

// issue-0011 步驟 5：完整真實回合。同一個 scope 先交代事實，再問 A 或 B，
// 記錄棒數、工具呼叫與回答，判斷 SCN-001／003 的回答內容。
const FACTS_TURN: &str = "先記住目前狀態，不需要查證：排程 run #869 開始 20 秒後失敗，錯誤是 session lease unavailable for scope user:tg-915354960。升級 v0.22.1 後的 9 次正式執行中，只有 #863（1506 秒）和 #870（796 秒）成功，另外 6 次是 OpenCode 逾時。請用一句話確認你記住了。";

#[tokio::test]
#[ignore = "calls a real model through OpenCode"]
async fn real_followup_turns() {
    let model = probe_model();
    let samples: usize = std::env::var("WUKONG_PLANNER_PROBE_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    println!("model={model} samples={samples}");
    for (case, followup) in [CASES[0], CASES[1]] {
        for sample in 1..=samples {
            let workspace = tempfile::tempdir().unwrap();
            std::env::set_var("WUKONG_WORKSPACE", workspace.path());
            let db = tempfile::NamedTempFile::new().unwrap();
            let memory = wukong_memory::Memory::open(&format!("sqlite://{}", db.path().display()))
                .await
                .unwrap();
            let backend = AgentCliBackend {
                command: vec![
                    "opencode".into(),
                    "run".into(),
                    "--model".into(),
                    model.clone(),
                ],
                workspace: Some(workspace.path().to_path_buf()),
            };
            let cfg = wukong_gateway::config::GatewayConfig {
                scope: format!("project:probe-{case}-{sample}"),
                db_url: String::new(),
                agent_command: vec![],
                default_model: None,
                planner_preferences: None,
                thinking: false,
                recall_top_k: 5,
                stream: true,
            };
            for (turn, input) in [("facts", FACTS_TURN), (case, followup)] {
                let started = Instant::now();
                let mut roles = Vec::new();
                let mut tools = Vec::new();
                let out = wukong_runtime::run_turn(
                    &memory,
                    &backend,
                    &cfg,
                    input,
                    &mut |event| {
                        if let wukong_gateway::StreamEvent::ToolUse(name) = event {
                            tools.push(name);
                        }
                    },
                    &mut |role| roles.push(role.name()),
                )
                .await
                .unwrap();
                println!(
                    "case={case} sample={sample} turn={turn} roles={} tools=[{}] secs={:.1}\n  answer={}",
                    roles.join(","),
                    tools.join(","),
                    started.elapsed().as_secs_f64(),
                    out.text.replace('\n', " ")
                );
            }
        }
    }
}
