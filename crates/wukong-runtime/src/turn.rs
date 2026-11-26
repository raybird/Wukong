use crate::persona;
use thiserror::Error;
use wukong_gateway::backend::{AgentAttachment, AgentRequest, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_memory::{
    Memory, MemoryItem, MemoryKind, MemoryRecord, RecallExplanation, RecallHit, RecallMode,
    RecallQuery, RememberInput,
};
use wukong_orchestrator::Role;

/// All errors produced by the unified turn.
#[derive(Debug, Error)]
pub enum WukongError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("backend error: {0}")]
    Backend(#[from] wukong_gateway::GatewayError),
}

/// The result of one unified turn.
#[derive(Debug, Clone)]
pub struct TurnOutput {
    pub text: String,
}

pub struct ObservedStep<'a> {
    pub role: Role,
    pub skill_name: Option<&'a str>,
    pub output: &'a str,
}

/// One unified Wukong turn（2026-11-26，#45）：取得 session lease → recall → 以 primary agent
/// `wukong` 送出一次（派工由 V2 的 subagent 處理，gateway 等背景工作清空才回傳）→ 輸出為空時
/// 補送一次修復提示 → remember。
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
) -> Result<TurnOutput, WukongError> {
    // Delegate with a no-op step observer so existing callers (CLI, Telegram,
    // Scheduler) and their tests keep an unchanged signature.
    run_turn_observed(
        memory,
        backend,
        cfg,
        input,
        on_event,
        on_role,
        &mut |_, _| {},
    )
    .await
}

pub async fn run_turn_with_attachments(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    attachments: Vec<AgentAttachment>,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
) -> Result<TurnOutput, WukongError> {
    let mut on_role = |_| {};
    run_turn_observed_with_attachments(
        memory,
        backend,
        cfg,
        input,
        attachments,
        on_event,
        &mut on_role,
        &mut |_, _| {},
    )
    .await
}

/// Same as [`run_turn`], with an `on_step(role, output)` observer for intermediate
/// results. 2026-11-26（#45 SCN-002）：由 V2 的 subagent 回報觸發（`StreamEvent::SubagentReported`）。
pub async fn run_turn_observed(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(Role, &str),
) -> Result<TurnOutput, WukongError> {
    run_turn_observed_with_attachments(
        memory,
        backend,
        cfg,
        input,
        Vec::new(),
        on_event,
        on_role,
        on_step,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_observed_with_attachments(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    attachments: Vec<AgentAttachment>,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(Role, &str),
) -> Result<TurnOutput, WukongError> {
    run_turn_traced_with_attachments(
        memory,
        backend,
        cfg,
        input,
        attachments,
        on_event,
        on_role,
        &mut |step| on_step(step.role, step.output),
    )
    .await
}

pub async fn run_turn_traced(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep<'_>),
) -> Result<TurnOutput, WukongError> {
    run_turn_traced_with_attachments(
        memory,
        backend,
        cfg,
        input,
        Vec::new(),
        on_event,
        on_role,
        on_step,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_traced_with_attachments(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    attachments: Vec<AgentAttachment>,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    // 2026-11-26（#45 SCN-002）：planner 與接力鏈已移除，角色與產出改由 V2 的 subagent 事件觸發：
    // subagent 開始時 `on_role`，回報送回 primary 時 `on_step`。只認 Wukong 的五個角色。
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep<'_>),
) -> Result<TurnOutput, WukongError> {
    let role_named = |agent: &str| Role::all().into_iter().find(|role| role.name() == agent);
    let mut on_event = |event: wukong_gateway::StreamEvent| {
        match &event {
            wukong_gateway::StreamEvent::SubagentStarted { agent } => {
                if let Some(role) = role_named(agent) {
                    on_role(role);
                }
            }
            wukong_gateway::StreamEvent::SubagentReported { agent, output } => {
                if let Some(role) = role_named(agent) {
                    on_step(ObservedStep {
                        role,
                        skill_name: None,
                        output,
                    });
                }
            }
            _ => {}
        }
        on_event(event);
    };
    let on_event: &mut dyn FnMut(wukong_gateway::StreamEvent) = &mut on_event;
    // 2026-10-06：入口附加的產出目錄只給 agent；召回與記憶只用使用者原文，
    // 否則每則輸入都會因為共用的區塊文字而「關聯」到所有過去的記憶。
    let user_text = input
        .split(&format!("\n\n{}", persona::ARTIFACT_DIR_HEADER))
        .next()
        .unwrap_or(input);

    // 2026-11-26（#45 SCN-012）：先取得 lease 再做任何事；同一個 scope 已有回合在執行時，
    // 這裡就被拒絕，不花任何 LLM 呼叫。
    let preparation = crate::session::prepare_final_session(memory, backend, cfg).await?;
    let stored = preparation.session_id.clone();
    let rotated_from = preparation.rotated_from.clone();
    let session_owner = preparation.owner.clone();
    let session_lease_secs = preparation.lease_secs;
    // 2026-10-20（issue-0012）：拿到自己的 lease 之後，告訴 agent 有哪些回合同時在
    // 這個工作目錄執行，但不阻擋任何一方。
    let concurrent_notice =
        notice_for_other_turns(memory.active_session_leases().await, &cfg.scope);
    let work = async {
        // 2026-10-06：回合接續 session，最近的對話已在其中；只注入與輸入相關的記憶。
        let recall = memory
            .recall(RecallQuery {
                query: user_text.to_string(),
                top_k: cfg.recall_top_k,
                scope: Some(cfg.scope.clone()),
                mode: RecallMode::Relevant,
            })
            .await?;
        // 2026-11-03（issue-0028）：輪替的回合沒有舊 session 可接續，帶上上一回合，
        // 「剛剛那個」之類的追問才看得懂。接續 session 的回合不帶（#45 SCN-004）。
        let hits = match rotated_from {
            Some(_) => with_previous_turn(
                &previous_turn_records(memory, &cfg.scope).await?,
                &recall.data,
            ),
            None => recall.data,
        };
        // 2026-11-26（#45）：人格、派工規則與 subagent 的角色卡在 agent 定義裡
        // （crate::agent_assets），技能交給 V2 的 skill 機制；這裡不再附角色卡。
        let mut prompt = wukong_gateway::prompt::compose_prompt(&hits, input);
        // 2026-11-26（#45 SCN-006～008）：V2 backend 的輸出要求、排程能力與長期記憶能力在 agent
        // 定義裡，scope 在 session 指令層，都不進 user message；CLI backend 沒有指令層，照舊附在
        // prompt。
        let session_instructions = if backend.supports_session_instructions() {
            std::collections::BTreeMap::from([(
                persona::SCOPE_INSTRUCTION_KEY.to_string(),
                persona::current_scope_instruction(&cfg.scope),
            )])
        } else {
            prompt.push_str("\n\n");
            prompt.push_str(persona::final_answer_directive());
            prompt.push_str("\n\n");
            prompt.push_str(&persona::scheduling_capability_hint(
                &cfg.scope,
                &persona::scheduling_bin(),
            ));
            if persona::memoria_hint_enabled() {
                prompt.push_str("\n\n");
                prompt.push_str(persona::memoria_capability_hint());
            }
            if user_text.len() < input.len() {
                prompt.push_str("\n\n");
                prompt.push_str(persona::file_interaction_rules());
            }
            std::collections::BTreeMap::new()
        };
        if let Some(notice) = &concurrent_notice {
            prompt.push_str("\n\n");
            prompt.push_str(notice);
        }
        let request = |prompt: String, session_id: Option<String>| AgentRequest {
            prompt,
            session_id,
            thinking: cfg.thinking,
            model: cfg.default_model.clone(),
            agent: Some(crate::agent_assets::PRIMARY_AGENT.to_string()),
            tool_overrides: std::collections::BTreeMap::new(),
            attachments: attachments.clone(),
            session_instructions: session_instructions.clone(),
        };

        let resp = backend
            .run_streaming(request(prompt.clone(), stored.clone()), on_event)
            .await?;
        let mut captured_session = resp.session_id.clone().or_else(|| stored.clone());
        // 最終輸出是空的（例如只做了工具呼叫就結束）時，在同一個 session 補送一次修復提示，
        // 使用者才不會收到空白回覆，記憶也不會留下空的 Assistant。
        let answer = if !resp.text.trim().is_empty() {
            resp.text
        } else {
            let repair = backend
                .run_streaming(
                    request(
                        append_empty_output_repair_directive(prompt),
                        captured_session.clone(),
                    ),
                    on_event,
                )
                .await?;
            if let Some(id) = repair.session_id {
                captured_session = Some(id);
            }
            if repair.text.trim().is_empty() {
                "(本回合未產生文字輸出)".to_string()
            } else {
                repair.text
            }
        };

        // 2026-10-06：session 會跨回合沿用，不能當防重複 key（否則只有第一回合被記住）；
        // 每次 run_turn 產生自己的識別碼，同一回合只寫一次。
        let turn_key = format!("scope:{}:turn:{}", cfg.scope, uuid::Uuid::new_v4());

        memory
            .remember(RememberInput {
                scope: cfg.scope.clone(),
                session_id: captured_session.clone(),
                items: vec![
                    MemoryItem {
                        kind: MemoryKind::Event,
                        text: format!("User: {user_text}"),
                        importance: None,
                        dedupe_key: Some(format!("runtime:{turn_key}:user")),
                    },
                    MemoryItem {
                        kind: MemoryKind::Event,
                        text: format!("Assistant: {answer}"),
                        importance: None,
                        dedupe_key: Some(format!("runtime:{turn_key}:assistant")),
                    },
                ],
            })
            .await?;

        memory
            .acquire_agent_session_lease(&cfg.scope, &session_owner, session_lease_secs)
            .await?
            .ok_or_else(|| {
                WukongError::Backend(wukong_gateway::GatewayError::AgentFailed {
                    code: None,
                    stderr: format!("session lease unavailable for scope {}", cfg.scope),
                })
            })?;
        memory
            .finish_agent_session_turn(&cfg.scope, &session_owner, captured_session.as_deref())
            .await?
            .ok_or_else(|| {
                WukongError::Backend(wukong_gateway::GatewayError::AgentFailed {
                    code: None,
                    stderr: format!("session lease lost for scope {}", cfg.scope),
                })
            })?;
        if let Some(old_id) = rotated_from.as_deref() {
            if captured_session.as_deref() != Some(old_id) {
                let new_id = captured_session.as_deref().unwrap_or("<none>");
                eprintln!(
                    "session_rotated scope={} old_session_id={} new_session_id={}",
                    cfg.scope, old_id, new_id
                );
                if let Err(error) = backend.delete_session(old_id).await {
                    eprintln!(
                    "session_rotation_cleanup_failed scope={} old_session_id={} new_session_id={}: {error}",
                    cfg.scope, old_id, new_id
                );
                }
            }
        }

        Ok(TurnOutput { text: answer })
    };
    // 2026-10-21（issue-0013）：單次 backend 呼叫可以比 session lease 久，執行期間續租，
    // 別的回合才不會在中途拿走 lease、和本回合共用同一個 session。
    let result = crate::session::hold_session_lease(
        memory,
        &cfg.scope,
        &session_owner,
        session_lease_secs,
        work,
    )
    .await;

    if result.is_err() {
        let _ = memory
            .release_agent_session_lease(&cfg.scope, &session_owner)
            .await;
    }
    result
}

fn append_empty_output_repair_directive(mut prompt: String) -> String {
    prompt.push_str("\n\n[修復回覆]\n");
    prompt.push_str(
        "上一輪沒有產生任何可回覆文字，可能是工具不可用、權限被拒，或只完成了工具呼叫。\n",
    );
    prompt.push_str(
        "這次不要呼叫工具，也不要嘗試讀取環境；請直接根據使用者原問題與已知上下文，用繁體中文給出可交付的文字回覆。",
    );
    prompt
}

/// 其他仍持有 session lease 的 scope 就是同時在執行的回合。告知只是軟性提示：
/// 查詢失敗就當作沒有其他回合，不能讓回合失敗——這時本回合的 lease 已經取得，
/// 在 `result` 區塊外失敗會讓它殘留到過期（issue-0012 審查 M-1）。
fn notice_for_other_turns(
    leases: Result<Vec<String>, wukong_memory::MemoryError>,
    own_scope: &str,
) -> Option<String> {
    let others: Vec<String> = match leases {
        Ok(scopes) => scopes.into_iter().filter(|s| s != own_scope).collect(),
        Err(err) => {
            eprintln!("concurrent_turn_lookup_failed scope={own_scope}: {err}");
            return None;
        }
    };
    (!others.is_empty())
        .then(|| persona::concurrent_turns_notice(&others, &persona::scheduling_bin()))
}

/// The scope's previous turn (its two newest turn memories), oldest first.
/// Called before this turn is remembered.
async fn previous_turn_records(
    memory: &Memory,
    scope: &str,
) -> Result<Vec<MemoryRecord>, WukongError> {
    let mut previous = memory
        .records(Some(scope), Some(MemoryKind::Event), 2)
        .await?
        .records;
    previous.reverse();
    Ok(previous)
}

/// `hits` plus the previous turn, skipping any already present.
fn with_previous_turn(previous: &[MemoryRecord], hits: &[RecallHit]) -> Vec<RecallHit> {
    let mut out = hits.to_vec();
    for record in previous {
        if out.iter().any(|hit| hit.id == record.id) {
            continue;
        }
        out.push(RecallHit {
            id: record.id,
            scope: record.scope.clone(),
            kind: record.kind,
            text: record.text.clone(),
            score: 0.0,
            explanation: RecallExplanation {
                lexical: 0.0,
                semantic: 0.0,
                relevance: 0.0,
                decay: 0.0,
                importance: record.importance,
                recall_bonus: 0.0,
                age_seconds: 0,
                recall_count: record.recall_count,
                source_signals: vec!["previous_turn".to_string()],
            },
        });
    }
    out
}

/// Send a raw `/compact` message to a specific opencode session (no planner,
/// no persona) and return its text reply.
pub async fn run_turn_session_passthrough(
    backend: &impl AiBackend,
    session_id: &str,
    command: &str,
) -> Result<String, WukongError> {
    let resp = if command == "/compact" {
        backend.compact_session(session_id, None).await?
    } else {
        backend
            .run_streaming(
                AgentRequest {
                    prompt: command.to_string(),
                    session_id: Some(session_id.to_string()),
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: std::collections::BTreeMap::new(),
                    attachments: Vec::new(),
                    session_instructions: Default::default(),
                },
                &mut |_| {},
            )
            .await?
    };
    Ok(resp.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentAttachment, AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        prompts: Mutex<Vec<String>>,
        session_ids: Mutex<Vec<Option<String>>>,
        models: Mutex<Vec<Option<String>>>,
        agents: Mutex<Vec<Option<String>>>,
        attachments: Mutex<Vec<Vec<AgentAttachment>>>,
        persistent_error: Mutex<bool>,
        deleted_sessions: Mutex<Vec<String>>,
        response_session_ids: Mutex<VecDeque<String>>,
        fail_on_call: Mutex<Option<usize>>,
        session_layer: bool,
        instructions: Mutex<Vec<std::collections::BTreeMap<String, String>>>,
        /// 第一次串流呼叫在回覆文字之前送出的事件（模擬 V2 的 subagent 事件）。
        events: Mutex<Vec<wukong_gateway::StreamEvent>>,
    }

    impl MockBackend {
        fn new(replies: &[&str]) -> MockBackend {
            MockBackend {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
                session_ids: Mutex::new(Vec::new()),
                models: Mutex::new(Vec::new()),
                agents: Mutex::new(Vec::new()),
                attachments: Mutex::new(Vec::new()),
                persistent_error: Mutex::new(false),
                deleted_sessions: Mutex::new(Vec::new()),
                response_session_ids: Mutex::new(VecDeque::new()),
                fail_on_call: Mutex::new(None),
                session_layer: false,
                instructions: Mutex::new(Vec::new()),
                events: Mutex::new(Vec::new()),
            }
        }

        fn with_events(self, events: Vec<wukong_gateway::StreamEvent>) -> MockBackend {
            *self.events.lock().unwrap() = events;
            self
        }

        /// 模擬 OpenCode V2 server：支援 session 指令層。
        fn with_session_layer(mut self) -> MockBackend {
            self.session_layer = true;
            self
        }

        /// 第 `call` 次（從 0 起算）呼叫 `run` 時失敗。
        fn failing_on_call(self, call: usize) -> MockBackend {
            *self.fail_on_call.lock().unwrap() = Some(call);
            self
        }

        fn with_response_session_ids(self, ids: &[&str]) -> MockBackend {
            *self.response_session_ids.lock().unwrap() =
                ids.iter().map(|id| id.to_string()).collect();
            self
        }

        fn with_persistent_error(self) -> MockBackend {
            *self.persistent_error.lock().unwrap() = true;
            self
        }
    }

    impl AiBackend for MockBackend {
        fn supports_session_instructions(&self) -> bool {
            self.session_layer
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            for event in std::mem::take(&mut *self.events.lock().unwrap()) {
                on_event(event);
            }
            let resp = self.run(req).await?;
            on_event(wukong_gateway::StreamEvent::Text(resp.text.clone()));
            Ok(resp)
        }

        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            let has_session = req.session_id.is_some();
            self.instructions
                .lock()
                .unwrap()
                .push(req.session_instructions.clone());
            let call = self.prompts.lock().unwrap().len();
            self.prompts.lock().unwrap().push(req.prompt);
            self.session_ids.lock().unwrap().push(req.session_id);
            self.models.lock().unwrap().push(req.model);
            self.agents.lock().unwrap().push(req.agent);
            self.attachments.lock().unwrap().push(req.attachments);
            if *self.fail_on_call.lock().unwrap() == Some(call) {
                return Err(GatewayError::AgentFailed {
                    code: Some(500),
                    stderr: "scripted failure".to_string(),
                });
            }
            if has_session && *self.persistent_error.lock().unwrap() {
                return Err(GatewayError::AgentFailed {
                    code: Some(500),
                    stderr: "persistent session failed".to_string(),
                });
            }
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            let session_id = self
                .response_session_ids
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| "ses_new".to_string());
            Ok(AgentResponse {
                text,
                session_id: Some(session_id),
            })
        }

        async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
            self.deleted_sessions
                .lock()
                .unwrap()
                .push(session_id.to_string());
            Ok(())
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn passthrough_sends_requested_command_to_session() {
        let backend = MockBackend::new(&["ok"]);

        let text = run_turn_session_passthrough(&backend, "ses_42", "/compact")
            .await
            .unwrap();

        assert_eq!(text, "ok");
        assert_eq!(backend.prompts.lock().unwrap()[0], "/compact");
        assert_eq!(
            backend.session_ids.lock().unwrap()[0],
            Some("ses_42".to_string())
        );
    }

    #[tokio::test]
    async fn compact_passthrough_uses_backend_session_control() {
        struct NativeCompactBackend {
            called: Mutex<bool>,
        }

        impl AiBackend for NativeCompactBackend {
            async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
                panic!("compact should not use ordinary run")
            }

            async fn compact_session(
                &self,
                session_id: &str,
                _model: Option<&str>,
            ) -> Result<AgentResponse, GatewayError> {
                assert_eq!(session_id, "ses_42");
                *self.called.lock().unwrap() = true;
                Ok(AgentResponse {
                    text: "native compact".to_string(),
                    session_id: Some(session_id.to_string()),
                })
            }
        }

        let backend = NativeCompactBackend {
            called: Mutex::new(false),
        };
        let text = run_turn_session_passthrough(&backend, "ses_42", "/compact")
            .await
            .unwrap();
        assert_eq!(text, "native compact");
        assert!(*backend.called.lock().unwrap());
    }

    // issue-0012 審查 M-1：查詢失敗不能讓回合失敗；查不到就不告知。
    #[test]
    fn notice_lookup_failure_is_ignored() {
        assert_eq!(
            notice_for_other_turns(
                Err(wukong_memory::MemoryError::InvalidQuery("boom".to_string())),
                "user:tg-42",
            ),
            None
        );
        assert_eq!(
            notice_for_other_turns(Ok(vec!["user:tg-42".to_string()]), "user:tg-42"),
            None
        );
    }

    #[tokio::test]
    async fn failed_turn_releases_session_lease() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_existing")
            .await
            .unwrap();
        let backend = MockBackend::new(&["answer"]).with_persistent_error();

        let error = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "hello",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap_err();

        // 上游錯誤照舊往外傳（#45 SCN-012）
        assert!(matches!(error, WukongError::Backend(_)), "{error:?}");
        assert!(error.to_string().contains("persistent session failed"));
        let retry = mem
            .acquire_agent_session_lease("project:T", "retry", 300)
            .await
            .unwrap();
        assert!(retry.is_some());
    }

    #[tokio::test]
    async fn run_turn_passes_default_model() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["answer"]);
        let mut cfg = test_cfg("project:T");
        cfg.default_model = Some("opencode/deepseek-v4-flash-free".to_string());

        run_turn(&mem, &backend, &cfg, "hello", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();

        let models = backend.models.lock().unwrap();
        assert_eq!(
            models.last().and_then(Clone::clone).as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    // #45 SCN-002：V2 的 subagent 事件觸發入口的角色與產出回呼；不是 Wukong 五個角色的 agent
    // （例如 V2 內建的 general）不觸發。事件本身照樣轉給入口。
    #[tokio::test]
    async fn subagent_events_drive_the_role_and_step_callbacks() {
        use wukong_gateway::StreamEvent;
        let mem = open_memory().await;
        let backend = MockBackend::new(&["done"]).with_events(vec![
            StreamEvent::SubagentStarted {
                agent: "fixer".to_string(),
            },
            StreamEvent::SubagentStarted {
                agent: "general".to_string(),
            },
            StreamEvent::SubagentReported {
                agent: "general".to_string(),
                output: "ignored".to_string(),
            },
            StreamEvent::SubagentReported {
                agent: "fixer".to_string(),
                output: "已建立 hello.txt".to_string(),
            },
        ]);
        let mut roles = Vec::new();
        let mut steps = Vec::new();
        let mut forwarded = 0;
        let out = run_turn_traced(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "建立 hello.txt",
            &mut |ev| {
                if matches!(
                    ev,
                    StreamEvent::SubagentStarted { .. } | StreamEvent::SubagentReported { .. }
                ) {
                    forwarded += 1;
                }
            },
            &mut |role| roles.push(role),
            &mut |step| {
                steps.push((
                    step.role,
                    step.skill_name.is_none(),
                    step.output.to_string(),
                ))
            },
        )
        .await
        .unwrap();
        assert_eq!(out.text, "done");
        assert_eq!(roles, [Role::Fixer]);
        assert_eq!(steps, [(Role::Fixer, true, "已建立 hello.txt".to_string())]);
        assert_eq!(forwarded, 4);
    }

    // #45 SCN-013：空輸出補送的修復提示也指定同一個模型與 agent；subagent 沿用 session 選取的
    // 模型（probe-subagent.md），所以回合裡每次送出都要帶上它。
    #[tokio::test]
    async fn the_repair_prompt_keeps_the_turn_model() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["", "answer"]).with_response_session_ids(&["ses_turn"]);
        let mut cfg = test_cfg("project:T");
        cfg.default_model = Some("opencode/deepseek-v4-flash-free".to_string());

        run_turn(&mem, &backend, &cfg, "hello", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();

        let models = backend.models.lock().unwrap().clone();
        assert_eq!(models.len(), 2);
        assert!(models
            .iter()
            .all(|m| m.as_deref() == Some("opencode/deepseek-v4-flash-free")));
        assert!(backend
            .agents
            .lock()
            .unwrap()
            .iter()
            .all(|a| a.as_deref() == Some(crate::agent_assets::PRIMARY_AGENT)));
    }

    #[tokio::test]
    async fn run_turn_with_attachments_forwards_paths_to_backend() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["answer"]);
        let cfg = test_cfg("project:T");
        let attachments = vec![AgentAttachment {
            path: std::path::PathBuf::from("/tmp/report.pdf"),
            original_name: "report.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
        }];

        let _ = run_turn_with_attachments(
            &mem,
            &backend,
            &cfg,
            "請分析",
            attachments.clone(),
            &mut |_| {},
        )
        .await
        .unwrap();

        let requests = backend.attachments.lock().unwrap();
        assert_eq!(requests.last().unwrap(), &attachments);
    }

    fn test_cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            thinking: true,
            recall_top_k: 5,
            stream: true,
        }
    }

    /// 2026-11-26（#45 SCN-005）：技能改由 V2 的 skill 機制提供（工作區的
    /// `.opencode/skills/`，模型依描述自己載入），prompt 不附技能規範指引，
    /// 回合也不再寫出 `.wukong/skills/superpowers`。
    #[tokio::test]
    async fn run_turn_leaves_skills_to_v2() {
        let workspace = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", workspace.path());
        let mem = open_memory().await;
        let backend = MockBackend::new(&["done"]);
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "fix the bug",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        assert_eq!(out.text, "done");
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(!prompts[0].contains("[技能規範指引]"), "{}", prompts[0]);
        assert!(!prompts[0].contains("SKILL.md"), "{}", prompts[0]);
        assert!(!workspace.path().join(".wukong").exists());
        std::env::remove_var("WUKONG_WORKSPACE");
    }

    #[tokio::test]
    async fn run_turn_carries_the_scheduling_capability() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["f2"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "schedule it",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("[排程能力]"));
        assert!(prompts[0].contains("schedule add-turn"));
        assert!(prompts[0].contains("這個對話的 scope 是 `project:T`"));
        // 不支援 session 指令層的 backend（CLI）不收 entry，scope 照舊在 prompt 裡。
        assert!(backend.instructions.lock().unwrap()[0].is_empty());
    }

    // #45 SCN-007：V2 backend 的 scope 放在 session 指令層，排程能力的固定說明在 agent 定義，
    // 兩者都不進 user message；修復提示帶同一份 entry。
    #[tokio::test]
    async fn the_scope_goes_to_the_session_layer_not_the_prompt() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["", "f2"])
            .with_response_session_ids(&["ses_turn"])
            .with_session_layer();
        run_turn(
            &mem,
            &backend,
            &test_cfg("user:tg-42"),
            "schedule it",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let prompts = backend.prompts.lock().unwrap().clone();
        assert_eq!(prompts.len(), 2);
        for prompt in &prompts {
            // #45 SCN-006：輸出要求也在 agent 定義裡。
            assert!(!prompt.contains("[輸出要求]"), "{prompt}");
            assert!(!prompt.contains("[排程能力]"), "{prompt}");
            assert!(!prompt.contains("user:tg-42"), "{prompt}");
        }
        let instructions = backend.instructions.lock().unwrap().clone();
        assert_eq!(instructions.len(), 2);
        for entries in &instructions {
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert!(
                entries["wukong.scope"].contains("`user:tg-42`"),
                "{entries:?}"
            );
        }
    }

    /// Both the off and on cases live in one test on purpose: `WUKONG_MEMORIA_HINT`
    /// is process-global, so two tests toggling it would race under the default
    /// parallel harness and the failure would look like flakiness rather than a
    /// shared-state bug.
    ///
    /// The off case is the one that matters most. Most deployments do not enable
    /// the Memoria overlay, and advertising a CLI that is not on the agent's PATH
    /// is worse than silence — it would try, fail, and surface the failure to the
    /// user. Asserting only the on case would let a default-on regression ship.
    #[tokio::test]
    async fn run_turn_advertises_memoria_only_when_enabled() {
        const MARKER: &str = "[長期記憶能力]";

        std::env::remove_var("WUKONG_MEMORIA_HINT");
        let mem = open_memory().await;
        let backend = MockBackend::new(&["f2"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "remember this",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        {
            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 1);
            assert!(
                prompts.iter().all(|p| !p.contains(MARKER)),
                "預設不得宣傳 memoria：多數部署沒有啟用 overlay，agent 的 PATH 上沒有這個 CLI"
            );
        }

        std::env::set_var("WUKONG_MEMORIA_HINT", "1");
        let mem = open_memory().await;
        let backend = MockBackend::new(&["f2"]);
        let result = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "remember this",
            &mut |_| {},
            &mut |_| {},
        )
        .await;
        // #45 SCN-008：V2 backend 的長期記憶能力在 agent 定義裡，開啟時也不進 user message。
        let v2_mem = open_memory().await;
        let v2 = MockBackend::new(&["f2"]).with_session_layer();
        let v2_result = run_turn(
            &v2_mem,
            &v2,
            &test_cfg("project:T"),
            "remember this",
            &mut |_| {},
            &mut |_| {},
        )
        .await;
        std::env::remove_var("WUKONG_MEMORIA_HINT");
        result.unwrap();
        v2_result.unwrap();
        assert!(v2
            .prompts
            .lock()
            .unwrap()
            .iter()
            .all(|p| !p.contains(MARKER)));

        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains(MARKER), "開啟時回合應該拿到能力區塊");
        assert!(prompts[0].contains("memoria recall"));
        assert!(prompts[0].contains("memoria feedback"));
        // 分工必須寫在 prompt 裡：少了這句 agent 會把對話鏡射進去，把真正的決策稀釋掉。
        assert!(
            prompts[0].contains("不需要"),
            "必須說明對話已自動保存、不要重複寫入"
        );
    }

    // 2026-11-26（#45 SCN-001）：不另跑規劃，以 primary agent `wukong` 送出一次；人格與角色卡
    // 在 agent 定義裡，prompt 不帶角色卡。
    #[tokio::test]
    async fn a_turn_is_one_call_to_the_wukong_agent_and_is_remembered() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["done"]);
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "fix the bug",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        assert_eq!(out.text, "done");
        assert_eq!(
            backend.agents.lock().unwrap().clone(),
            vec![Some("wukong".to_string())]
        );
        let prompts = backend.prompts.lock().unwrap().clone();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("fix the bug"));
        for role in Role::all() {
            assert!(!prompts[0].contains(role.card()), "{}", prompts[0]);
        }
        assert!(!prompts[0].contains("[前序協作]"));

        let r = mem
            .recall(RecallQuery {
                query: "fix the bug".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: fix the bug")));
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: done")));
    }

    // 2026-11-26（#45 SCN-012）：同一個 scope 已有回合在執行時，新的回合在任何 LLM 呼叫之前就被拒絕。
    #[tokio::test]
    async fn a_turn_rejected_by_the_lease_makes_no_backend_call() {
        let mem = open_memory().await;
        mem.acquire_agent_session_lease("project:T", "running-turn", 900)
            .await
            .unwrap()
            .unwrap();
        let backend = MockBackend::new(&["answer"]);

        let error = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "hello",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("project:T"), "{error}");
        assert!(backend.prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn run_turn_threads_the_stored_session() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        let backend = MockBackend::new(&["answer"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "hi",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        {
            let ids = backend.session_ids.lock().unwrap();
            assert_eq!(ids.clone(), vec![Some("ses_old".to_string())]);
        }
        assert_eq!(
            mem.agent_session("project:T").await.unwrap(),
            Some("ses_new".to_string())
        );
        let state = mem.agent_session_state("project:T").await.unwrap().unwrap();
        assert_eq!(state.turn_count, 1);
        let records = mem.records(Some("project:T"), None, 10).await.unwrap();
        assert_eq!(records.records.len(), 2);
        assert!(records
            .records
            .iter()
            .all(|record| record.session_id.as_deref() == Some("ses_new")));
    }

    const DAY_SECS: i64 = 24 * 60 * 60;

    /// scope 指向 `ses_old`、累積 20 個回合（原本的壓縮門檻），session 在 `days_ago` 天前
    /// 開始，上一回合是晚餐的問答。
    async fn memory_after_twenty_turns(days_ago: i64) -> Memory {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        mem.set_agent_session_started(
            "project:T",
            "ses_old",
            crate::util::now_unix() - days_ago * DAY_SECS,
        )
        .await
        .unwrap();
        for _ in 0..20 {
            mem.acquire_agent_session_lease("project:T", "seed", 300)
                .await
                .unwrap();
            mem.finish_agent_session_turn("project:T", "seed", Some("ses_old"))
                .await
                .unwrap();
        }
        remember_turn(&mem, "project:T", "晚餐吃什麼", "吃拉麵").await;
        mem
    }

    // issue-0028 SCN-004：開始超過 7 天的 session 在回合開始時輪替（#45 SCN-010 起每個回合都
    // 檢查）；回合在新 session 執行、帶上一回合（#45 SCN-004 保留這條），取得新 session 後刪掉
    // 舊的，而且不做壓縮。
    #[tokio::test]
    async fn an_old_session_rotates_and_the_turn_sees_the_previous_turn() {
        let mem = memory_after_twenty_turns(8).await;
        let backend = MockBackend::new(&["answer"]).with_response_session_ids(&["ses_new"]);

        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "然後呢",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        assert_eq!(
            mem.agent_session("project:T").await.unwrap().as_deref(),
            Some("ses_new")
        );
        let prompts = backend.prompts.lock().unwrap();
        assert!(!prompts.iter().any(|prompt| prompt == "/compact"));
        let final_prompt = prompts.last().unwrap();
        assert!(final_prompt.contains("User: 晚餐吃什麼"), "{final_prompt}");
        assert!(final_prompt.contains("Assistant: 吃拉麵"), "{final_prompt}");
        assert_eq!(backend.session_ids.lock().unwrap().last().unwrap(), &None);
        assert!(backend
            .deleted_sessions
            .lock()
            .unwrap()
            .contains(&"ses_old".to_string()));
    }

    // #45 SCN-010：超過 20 回合、未滿 7 天的 session 不壓縮，續接同一個 session，回合不額外帶
    // 上一回合；回合只呼叫 backend 一次。
    #[tokio::test]
    async fn a_young_session_past_twenty_turns_continues_without_compaction() {
        let mem = memory_after_twenty_turns(6).await;
        let backend = MockBackend::new(&["answer"]).with_response_session_ids(&["ses_old"]);

        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "然後呢",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        assert!(!prompts.iter().any(|prompt| prompt == "/compact"));
        let final_prompt = prompts.last().unwrap();
        assert!(!final_prompt.contains("晚餐吃什麼"), "{final_prompt}");
        assert_eq!(
            backend
                .session_ids
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .as_deref(),
            Some("ses_old")
        );
        assert!(!backend
            .deleted_sessions
            .lock()
            .unwrap()
            .contains(&"ses_old".to_string()));
    }

    // issue-0028 SCN-007：輪替的回合在取得新 session 之前失敗，舊 session 不刪、
    // scope 仍指向它。
    #[tokio::test]
    async fn a_rotation_turn_that_fails_keeps_the_old_session() {
        let mem = memory_after_twenty_turns(8).await;
        let backend = MockBackend::new(&[]).failing_on_call(0);

        let result = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "然後呢",
            &mut |_| {},
            &mut |_| {},
        )
        .await;

        assert!(result.is_err());
        // 走的是輪替：回合沒有帶舊 session，也沒有做 compaction。
        {
            let prompts = backend.prompts.lock().unwrap();
            assert!(!prompts.iter().any(|p| p == "/compact"));
            assert_eq!(backend.session_ids.lock().unwrap()[0], None);
        }
        assert!(!backend
            .deleted_sessions
            .lock()
            .unwrap()
            .contains(&"ses_old".to_string()));
        assert_eq!(
            mem.agent_session("project:T").await.unwrap().as_deref(),
            Some("ses_old")
        );
    }

    // 輸出為空時，在同一個 session、同一個 agent 補送一次修復提示（#45 SCN-012）。
    #[tokio::test]
    async fn run_turn_all_empty_repairs_with_text() {
        let mem = open_memory().await;
        let backend =
            MockBackend::new(&["", "直接回答原問題"]).with_response_session_ids(&["ses_turn"]);
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "你目前是什麼模型",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        assert_eq!(out.text, "直接回答原問題");
        let prompts = backend.prompts.lock().unwrap().clone();
        assert_eq!(prompts.len(), 2);
        let repair_prompt = &prompts[1];
        assert!(repair_prompt.contains("[修復回覆]"));
        assert!(repair_prompt.contains("不要呼叫工具"));
        assert!(repair_prompt.contains("你目前是什麼模型"));
        assert_eq!(
            backend.session_ids.lock().unwrap()[1].as_deref(),
            Some("ses_turn")
        );
        assert_eq!(backend.agents.lock().unwrap()[1].as_deref(), Some("wukong"));

        let r = mem
            .recall(RecallQuery {
                query: "直接回答原問題".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r
            .data
            .iter()
            .any(|h| h.text.contains("Assistant: 直接回答原問題")));
    }

    #[tokio::test]
    async fn run_turn_all_empty_repair_empty_returns_sentinel() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["", ""]);
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "go",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        assert_eq!(out.text, "(本回合未產生文字輸出)");
        assert_eq!(backend.prompts.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn run_turn_carries_the_final_answer_directive() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["f2"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "go",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("[輸出要求]"));
    }

    async fn remember_note(mem: &Memory, scope: &str, text: &str) {
        mem.remember(RememberInput {
            scope: scope.to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Note,
                text: text.to_string(),
                importance: None,
                dedupe_key: None,
            }],
        })
        .await
        .unwrap();
    }

    const UNRELATED: [&str; 3] = ["晚餐吃拉麵", "天氣晴朗", "週末去爬山"];

    // SCN-002: the final step resumes its session; recency alone is not relevance.
    #[tokio::test]
    async fn final_step_omits_memories_selected_only_by_recency() {
        let mem = open_memory().await;
        for text in UNRELATED {
            remember_note(&mem, "project:T", text).await;
        }
        let backend = MockBackend::new(&["answer"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "deploy the server",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let prompts = backend.prompts.lock().unwrap();
        let final_prompt = prompts.last().unwrap();
        assert!(!final_prompt.contains("[相關記憶]"));
        for text in UNRELATED {
            assert!(
                !final_prompt.contains(text),
                "{text} leaked into the final step"
            );
        }
    }

    // SCN-003: an older memory that matches the input still reaches the final step.
    #[tokio::test]
    async fn final_step_keeps_older_relevant_memory() {
        let mem = open_memory().await;
        remember_note(&mem, "project:T", "deploy port is 8787").await;
        for text in UNRELATED {
            remember_note(&mem, "project:T", text).await;
        }
        let backend = MockBackend::new(&["answer"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "which deploy port",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let prompts = backend.prompts.lock().unwrap();
        let final_prompt = prompts.last().unwrap();
        assert!(final_prompt.contains("(project:T) deploy port is 8787"));
        for text in UNRELATED {
            assert!(
                !final_prompt.contains(text),
                "{text} leaked into the final step"
            );
        }
    }

    async fn remember_turn(mem: &Memory, scope: &str, user: &str, assistant: &str) {
        for text in [format!("User: {user}"), format!("Assistant: {assistant}")] {
            mem.remember(RememberInput {
                scope: scope.to_string(),
                session_id: None,
                items: vec![MemoryItem {
                    kind: MemoryKind::Event,
                    text,
                    importance: None,
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
        }
    }

    // 2026-11-26（#45 SCN-004）：接續 session 的回合不補上一回合，最近的對話已在 session 裡。
    #[tokio::test]
    async fn a_continuing_session_gets_no_previous_turn() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        remember_turn(&mem, "project:T", "舊問題", "舊回答").await;
        remember_turn(&mem, "project:T", "晚餐吃什麼", "吃拉麵").await;
        let backend = MockBackend::new(&["f1"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "deploy the server",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        for text in ["晚餐吃什麼", "吃拉麵", "舊問題", "舊回答"] {
            assert!(!prompts[0].contains(text), "{text}: {}", prompts[0]);
        }
    }

    // issue-0012 SCN-001：另一個回合持有 lease 時，回合會被告知，兩個方向都成立。
    #[tokio::test]
    async fn other_running_turns_are_announced() {
        for (own, other, listed) in [
            (
                "user:tg-42",
                "agent:schedule-job-1",
                "- 排程 job-1（agent:schedule-job-1）\n",
            ),
            ("agent:schedule-job-1", "user:tg-42", "- 對話 user:tg-42\n"),
        ] {
            let mem = open_memory().await;
            mem.acquire_agent_session_lease(other, "other-turn", 900)
                .await
                .unwrap();
            let backend = MockBackend::new(&["f1"]);

            run_turn(
                &mem,
                &backend,
                &test_cfg(own),
                "改 notes.md",
                &mut |_| {},
                &mut |_| {},
            )
            .await
            .unwrap();

            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 1);
            for step in prompts.iter() {
                assert!(step.contains("[其他回合正在執行]\n"), "{own}: {step}");
                assert!(step.contains(listed), "{own}: {step}");
                assert!(step.contains("不要修改或提交不是本回合造成的變更"));
                assert!(!step.contains(&format!("{own}\n")), "{own} 不該列出自己");
            }
        }
    }

    // issue-0012 SCN-002／003：沒有其他有效 lease（含已釋放的、自己的）時 prompt 不變。
    #[tokio::test]
    async fn without_other_running_turns_prompts_are_unchanged() {
        // V2 backend：長期記憶能力不在 prompt 裡，比對不受 run_turn_advertises_memoria_only_when_enabled
        // 並行切換 WUKONG_MEMORIA_HINT 影響（CLI backend 的 prompt 會受影響）。
        async fn prompts(mem: &Memory) -> Vec<String> {
            let backend = MockBackend::new(&["f1"]).with_session_layer();
            run_turn(
                mem,
                &backend,
                &test_cfg("user:tg-42"),
                "改 notes.md",
                &mut |_| {},
                &mut |_| {},
            )
            .await
            .unwrap();
            let prompts = backend.prompts.lock().unwrap().clone();
            prompts
        }
        let fresh = open_memory().await;
        let released = open_memory().await;
        released
            .acquire_agent_session_lease("agent:schedule-job-1", "other-turn", 900)
            .await
            .unwrap();
        released
            .release_agent_session_lease("agent:schedule-job-1", "other-turn")
            .await
            .unwrap();

        let fresh = prompts(&fresh).await;
        assert!(fresh.iter().all(|p| !p.contains("[其他回合正在執行]")));
        assert_eq!(prompts(&released).await, fresh);
    }

    // SCN-001: the session is reused across turns, so it cannot be the dedupe
    // key; every turn is remembered, even one repeating the previous input.
    #[tokio::test]
    async fn every_turn_in_a_reused_session_is_remembered() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["first answer", "second answer"]);
        for expected in [2, 4] {
            run_turn(
                &mem,
                &backend,
                &test_cfg("project:T"),
                "same question",
                &mut |_| {},
                &mut |_| {},
            )
            .await
            .unwrap();
            let records = mem
                .records(Some("project:T"), Some(MemoryKind::Event), 10)
                .await
                .unwrap();
            assert_eq!(records.records.len(), expected);
        }

        let records = mem
            .records(Some("project:T"), Some(MemoryKind::Event), 10)
            .await
            .unwrap();
        let mut texts: Vec<&str> = records.records.iter().map(|r| r.text.as_str()).collect();
        texts.sort();
        assert_eq!(
            texts,
            [
                "Assistant: first answer",
                "Assistant: second answer",
                "User: same question",
                "User: same question",
            ]
        );
        assert!(records
            .records
            .iter()
            .all(|r| r.session_id.as_deref() == Some("ses_new")));
    }

    // SCN-002 (review-cc78101): same-language turns share stopwords and the
    // `User:`/`Assistant:` labels with any input; neither is relevance.
    #[tokio::test]
    async fn final_step_ignores_stopword_and_role_label_matches() {
        let mem = open_memory().await;
        remember_turn(
            &mem,
            "project:T",
            "what is the weather today",
            "It is sunny in the city",
        )
        .await;
        let backend = MockBackend::new(&["answer"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "deploy the server for this user",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let prompts = backend.prompts.lock().unwrap();
        let final_prompt = prompts.last().unwrap();
        assert!(!final_prompt.contains("[相關記憶]"));
        assert!(!final_prompt.contains("weather"));
        assert!(!final_prompt.contains("sunny"));
    }

    // SCN-009：Telegram 的產出目錄跟著送給 agent，但不進召回與使用者記憶。
    // #45 SCN-006：V2 backend 的 user message 只有產出目錄那一行，規則在 agent 定義裡；
    // CLI backend 另外附上規則。
    #[tokio::test]
    async fn file_rules_reach_the_prompt_but_not_memory_or_recall() {
        let dir_line = format!("\n\n{} /workspace/out", persona::ARTIFACT_DIR_HEADER);
        for v2 in [false, true] {
            let mem = open_memory().await;
            remember_note(&mem, "project:T", &format!("User: 晚餐吃拉麵{dir_line}")).await;
            let backend = if v2 {
                MockBackend::new(&["answer"]).with_session_layer()
            } else {
                MockBackend::new(&["answer"])
            };
            run_turn(
                &mem,
                &backend,
                &test_cfg("project:T"),
                &format!("部署伺服器的步驟{dir_line}"),
                &mut |_| {},
                &mut |_| {},
            )
            .await
            .unwrap();

            {
                let prompts = backend.prompts.lock().unwrap();
                let prompt = prompts.last().unwrap();
                assert!(prompt.contains("[產出目錄] /workspace/out"), "{prompt}");
                assert_eq!(prompt.contains("[檔案互動規則]"), !v2, "v2={v2}: {prompt}");
                assert!(!prompt.contains("晚餐吃拉麵"));
            }
            let events = mem
                .records(Some("project:T"), Some(MemoryKind::Event), 10)
                .await
                .unwrap();
            let users: Vec<&str> = events
                .records
                .iter()
                .map(|r| r.text.as_str())
                .filter(|t| t.starts_with("User:"))
                .collect();
            assert_eq!(users, ["User: 部署伺服器的步驟"], "v2={v2}");
        }
    }
}
