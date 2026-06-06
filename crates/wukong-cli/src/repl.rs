//! Interactive REPL: multi-turn loop sharing one Memory, with session
//! continuation after the first turn and minimal meta-commands.

use crate::{run_turn, WukongError};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;

/// What a single REPL input line means.
#[derive(Debug, PartialEq)]
pub enum LineAction {
    Exit,
    Skip,
    SetScope(String),
    Turn(String),
}

/// Classify one raw input line into an action.
pub fn classify_line(line: &str) -> LineAction {
    let t = line.trim();
    if t.is_empty() {
        return LineAction::Skip;
    }
    match t {
        "/exit" | "/quit" => LineAction::Exit,
        "/scope" => LineAction::Skip, // bare command, no scope given
        _ => {
            if let Some(rest) = t.strip_prefix("/scope ") {
                let s = rest.trim();
                if s.is_empty() {
                    LineAction::Skip
                } else {
                    LineAction::SetScope(s.to_string())
                }
            } else {
                LineAction::Turn(t.to_string())
            }
        }
    }
}

/// Run turns over a sequence of input lines (injectable for tests). Returns the
/// number of turns executed. `on_event` receives streamed events per turn;
/// `on_role` is called once per turn with the chosen role name (for the header).
pub async fn run_repl_loop<I>(
    memory: &Memory,
    backend: &impl AiBackend,
    base_cfg: &GatewayConfig,
    lines: I,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(&str),
) -> Result<usize, WukongError>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = base_cfg.clone();
    cfg.continue_session = false; // first turn fresh
    let mut turns = 0usize;
    for line in lines {
        match classify_line(&line) {
            LineAction::Exit => break,
            LineAction::Skip => continue,
            LineAction::SetScope(s) => {
                cfg.scope = s;
            }
            LineAction::Turn(input) => {
                // Forward the routed role (as name) to the loop's on_role sink.
                run_turn(memory, backend, &cfg, &input, on_event, &mut |r| {
                    on_role(r.name())
                })
                .await?;
                turns += 1;
                cfg.continue_session = true; // subsequent turns continue session
            }
        }
    }
    Ok(turns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        continue_flags: Mutex<Vec<bool>>,
    }
    impl MockBackend {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                continue_flags: Mutex::new(Vec::new()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.continue_flags.lock().unwrap().push(req.continue_session);
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            Ok(AgentResponse { text })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn cfg() -> GatewayConfig {
        GatewayConfig {
            scope: "project:T".to_string(),
            db_url: String::new(),
            agent_command: vec![],
            continue_args: vec![],
            continue_session: false,
            recall_top_k: 5,
            stream: true,
        }
    }

    #[test]
    fn classify_line_cases() {
        assert_eq!(classify_line("  "), LineAction::Skip);
        assert_eq!(classify_line("/exit"), LineAction::Exit);
        assert_eq!(classify_line("/quit"), LineAction::Exit);
        assert_eq!(classify_line("/scope global"), LineAction::SetScope("global".to_string()));
        assert_eq!(classify_line("/scope   "), LineAction::Skip);
        assert_eq!(classify_line("fix the bug"), LineAction::Turn("fix the bug".to_string()));
    }

    #[tokio::test]
    async fn loop_runs_turns_until_exit_and_continues_session() {
        let mem = open_memory().await;
        // route+execute per turn => 2 replies per turn; 2 turns then /exit.
        let backend = MockBackend::new(&["fixer", "ans1", "oracle", "ans2"]);
        let lines = vec![
            "first question".to_string(),
            "".to_string(), // skipped
            "second question".to_string(),
            "/exit".to_string(),
            "ignored after exit".to_string(),
        ];
        let mut roles = Vec::new();
        let turns = run_repl_loop(
            &mem,
            &backend,
            &cfg(),
            lines,
            &mut |_| {},
            &mut |r| roles.push(r.to_string()),
        )
        .await
        .unwrap();
        assert_eq!(turns, 2);
        // route reply "fixer" => Role::Fixer (name "fixer"); "oracle" => "oracle".
        assert_eq!(roles, vec!["fixer".to_string(), "oracle".to_string()]);
        // route always runs with continue_session=false (routing never continues
        // the agent session); only execute respects cfg.continue_session.
        // turn1: route=false, execute=false; turn2: route=false, execute=true.
        let flags = backend.continue_flags.lock().unwrap().clone();
        assert_eq!(flags, vec![false, false, false, true]);
    }

    #[tokio::test]
    async fn turn_persists_memory_across_loop() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        run_repl_loop(&mem, &backend, &cfg(), vec!["fix it".to_string(), "/exit".to_string()], &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
        let r = mem
            .recall(wukong_memory::RecallQuery {
                query: "fix it".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: wukong_memory::RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: fix it")));
    }
}
