use crate::persona;
use thiserror::Error;
use wukong_gateway::backend::{AgentAttachment, AgentRequest, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};
use wukong_orchestrator::{PlannerPreferenceHint, Role};
use wukong_skills::{find as find_skill, route_options};

/// All errors produced by the unified turn.
#[derive(Debug, Error)]
pub enum WukongError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] wukong_orchestrator::OrchestratorError),
    #[error("backend error: {0}")]
    Backend(#[from] wukong_gateway::GatewayError),
}

/// The result of one unified turn.
#[derive(Debug, Clone)]
pub struct TurnOutput {
    pub role: Role,
    pub text: String,
}

pub struct ObservedStep<'a> {
    pub role: Role,
    pub skill_name: Option<&'a str>,
    pub output: &'a str,
}

/// One unified Wukong turn: recall → route → execute (persona + role + memory)
/// → remember. Makes two backend calls (route, then execute).
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

/// Same as [`run_turn`], but additionally reports each *non-final* chain step's
/// output via `on_step(role, output)` the moment it completes (empty outputs are
/// skipped). The final step is delivered through the returned [`TurnOutput`], not
/// `on_step`, so a front-end can surface intermediate (helper) baton results
/// while the final answer stays the primary reply.
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
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep<'_>),
) -> Result<TurnOutput, WukongError> {
    let recall = memory
        .recall(RecallQuery {
            query: input.to_string(),
            top_k: cfg.recall_top_k,
            scope: Some(cfg.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await?;

    let preference_hint = planner_preference_hint(cfg);
    let steps = wukong_orchestrator::plan_skill_chain_with_preferences(
        backend,
        input,
        &route_options(),
        preference_hint.as_ref(),
    )
    .await?;

    let preparation = crate::session::prepare_final_session(memory, backend, cfg).await?;
    let stored = preparation.session_id.clone();
    let rotated_from = preparation.rotated_from.clone();
    let session_owner = preparation.owner.clone();
    let session_lease_secs = preparation.lease_secs;
    let result = async {
    let n_steps = steps.len();
    let mut prior: Vec<wukong_orchestrator::Outcome> = Vec::new();
    let mut captured_session: Option<String> = None;
    let mut final_repair: Option<(Role, Option<String>, String)> = None;
    let skill_root = crate::skill_assets::materialize_default_runtime_skills().map_err(|err| {
        WukongError::Backend(wukong_gateway::GatewayError::AgentFailed {
            code: None,
            stderr: format!("failed to prepare runtime skill assets: {err}"),
        })
    })?;
    for (i, step) in steps.into_iter().enumerate() {
        let role = step.role;
        on_role(role);
        let skill = step.skill_name.as_deref().and_then(find_skill);
        let augmented = format!("{input}{}", wukong_orchestrator::chain_context(&prior));
        let is_final = i + 1 == n_steps;
        let mut prompt =
            persona::build_prompt_with_skill(role, skill, &skill_root, &recall.data, &augmented);
        // Advertise the scheduling capability only on the user-facing final step,
        // so stateless helper steps never take a side-effecting schedule action.
        if is_final {
            prompt.push_str("\n\n");
            prompt.push_str(persona::final_answer_directive());
            prompt.push_str("\n\n");
            prompt.push_str(&persona::scheduling_capability_hint(
                &cfg.scope,
                &persona::scheduling_bin(),
            ));
        }
        let session_id = if is_final { stored.clone() } else { None };
        if is_final {
            final_repair = Some((role, session_id.clone(), prompt.clone()));
        }
        let resp = if is_final {
            backend
                .run_streaming(
                    AgentRequest {
                        prompt,
                        session_id,
                        thinking: cfg.thinking,
                        model: cfg.default_model.clone(),
                        agent: None,
                        tool_overrides: std::collections::BTreeMap::new(),
                        attachments: attachments.clone(),
                    },
                    on_event,
                )
                .await?
        } else {
            backend
                .run_streaming_ephemeral(
                    AgentRequest {
                        prompt,
                        session_id: None,
                        thinking: cfg.thinking,
                        model: None,
                        agent: None,
                        tool_overrides: std::collections::BTreeMap::new(),
                        attachments: attachments.clone(),
                    },
                    on_event,
                )
                .await?
        };
        if is_final {
            captured_session = resp.session_id.clone();
        }
        let text = resp.text;
        // Surface helper-baton output to observers as soon as it lands; the final
        // step is delivered via the returned TurnOutput, not on_step.
        if !is_final && !text.trim().is_empty() {
            on_step(ObservedStep {
                role,
                skill_name: step.skill_name.as_deref(),
                output: &text,
            });
        }
        prior.push(wukong_orchestrator::Outcome { role, output: text });
    }

    let last = prior
        .last()
        .cloned()
        .unwrap_or(wukong_orchestrator::Outcome {
            role: Role::Oracle,
            output: String::new(),
        });
    // The final step may be an executor that finished via tool calls without a
    // closing message, leaving its output empty. Fall back to the most recent
    // non-empty step so the user never receives a blank reply and memory/history
    // are not polluted with an empty Assistant entry.
    let answer = if last.output.trim().is_empty() {
        if let Some(existing) = prior
            .iter()
            .rev()
            .find(|o| !o.output.trim().is_empty())
            .cloned()
        {
            existing
        } else if let Some((role, session_id, prompt)) = final_repair {
            let repair = backend
                .run_streaming(
                    AgentRequest {
                        prompt: append_empty_output_repair_directive(prompt),
                        session_id: captured_session.clone().or(session_id),
                        thinking: cfg.thinking,
                        model: cfg.default_model.clone(),
                        agent: None,
                        tool_overrides: std::collections::BTreeMap::new(),
                        attachments: attachments.clone(),
                    },
                    on_event,
                )
                .await?;
            if let Some(id) = repair.session_id.as_deref() {
                captured_session = Some(id.to_string());
            }
            if !repair.text.trim().is_empty() {
                wukong_orchestrator::Outcome {
                    role,
                    output: repair.text,
                }
            } else {
                wukong_orchestrator::Outcome {
                    role,
                    output: "(本回合未產生文字輸出)".to_string(),
                }
            }
        } else {
            wukong_orchestrator::Outcome {
                role: last.role,
                output: "(本回合未產生文字輸出)".to_string(),
            }
        }
    } else {
        last
    };

    let turn_key = captured_session
        .clone()
        .or_else(|| stored.clone())
        .unwrap_or_else(|| format!("scope:{}:input:{}", cfg.scope, input));

    memory
        .remember(RememberInput {
            scope: cfg.scope.clone(),
            session_id: captured_session.clone().or_else(|| stored.clone()),
            items: vec![
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("User: {input}"),
                    importance: None,
                    dedupe_key: Some(format!("runtime:{turn_key}:user")),
                },
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("Assistant: {}", answer.output),
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
    let turn_session_id = captured_session.clone().or(stored.clone());
    memory
        .finish_agent_session_turn(&cfg.scope, &session_owner, turn_session_id.as_deref())
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

    Ok(TurnOutput {
        role: answer.role,
        text: answer.output,
    })
    }
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

fn parse_preferred_role(name: &str) -> Option<Role> {
    let normalized = name.trim().to_ascii_lowercase();
    Role::all()
        .into_iter()
        .find(|role| role.name() == normalized)
}

fn planner_preference_hint(cfg: &GatewayConfig) -> Option<PlannerPreferenceHint> {
    let prefs = cfg.planner_preferences.as_ref()?;
    let preferred_roles = prefs
        .preferred_roles
        .iter()
        .filter_map(|role| parse_preferred_role(role))
        .collect::<Vec<_>>();
    let preferred_skills = prefs
        .preferred_skills
        .iter()
        .filter(|skill| find_skill(skill).is_some())
        .cloned()
        .collect::<Vec<_>>();
    if preferred_roles.is_empty() && preferred_skills.is_empty() {
        None
    } else {
        Some(PlannerPreferenceHint {
            preferred_roles,
            preferred_skills,
        })
    }
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
        attachments: Mutex<Vec<Vec<AgentAttachment>>>,
        compact_error: Mutex<Option<i32>>,
        persistent_error: Mutex<bool>,
        deleted_sessions: Mutex<Vec<String>>,
        response_session_ids: Mutex<VecDeque<String>>,
    }

    impl MockBackend {
        fn new(replies: &[&str]) -> MockBackend {
            MockBackend {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
                session_ids: Mutex::new(Vec::new()),
                models: Mutex::new(Vec::new()),
                attachments: Mutex::new(Vec::new()),
                compact_error: Mutex::new(None),
                persistent_error: Mutex::new(false),
                deleted_sessions: Mutex::new(Vec::new()),
                response_session_ids: Mutex::new(VecDeque::new()),
            }
        }

        fn with_compact_error(replies: &[&str], code: i32) -> MockBackend {
            let backend = Self::new(replies);
            *backend.compact_error.lock().unwrap() = Some(code);
            backend
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
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            let is_compact = req.prompt == "/compact";
            let has_session = req.session_id.is_some();
            self.prompts.lock().unwrap().push(req.prompt);
            self.session_ids.lock().unwrap().push(req.session_id);
            self.models.lock().unwrap().push(req.model);
            self.attachments.lock().unwrap().push(req.attachments);
            if has_session && *self.persistent_error.lock().unwrap() {
                return Err(GatewayError::AgentFailed {
                    code: Some(500),
                    stderr: "persistent session failed".to_string(),
                });
            }
            if is_compact {
                if let Some(code) = self.compact_error.lock().unwrap().take() {
                    return Err(GatewayError::AgentFailed {
                        code: Some(code),
                        stderr: "compact failed".to_string(),
                    });
                }
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

    #[tokio::test]
    async fn pre_final_compaction_resets_turn_count_before_reuse() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["compacted"]);
        let cfg = test_cfg("project:T");
        mem.set_agent_session("project:T", "ses_1").await.unwrap();
        mem.acquire_agent_session_lease("project:T", "seed", 300)
            .await
            .unwrap();
        mem.finish_agent_session_turn("project:T", "seed", Some("ses_1"))
            .await
            .unwrap();

        let prepared = crate::session::prepare_final_session_with_policy(
            &mem,
            &backend,
            &cfg,
            crate::session::SessionPolicy {
                compact_every_turns: 1,
                lease_secs: 300,
            },
        )
        .await
        .unwrap();
        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert!(prepared.rotated_from.is_none());
        assert_eq!(
            mem.agent_session_state("project:T")
                .await
                .unwrap()
                .unwrap()
                .turn_count,
            0
        );
        assert!(backend
            .prompts
            .lock()
            .unwrap()
            .iter()
            .any(|prompt| prompt == "/compact"));
    }

    #[tokio::test]
    async fn failed_turn_releases_session_lease() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_existing")
            .await
            .unwrap();
        let backend = MockBackend::new(&["oracle", "answer"]).with_persistent_error();

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

        assert!(error.to_string().contains("persistent session failed"));
        let retry = mem
            .acquire_agent_session_lease("project:T", "retry", 300)
            .await
            .unwrap();
        assert!(retry.is_some());
    }

    #[tokio::test]
    async fn run_turn_passes_default_model_to_final_step() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "answer"]);
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

    #[tokio::test]
    async fn run_turn_with_attachments_forwards_paths_to_backend() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "answer"]);
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

    #[tokio::test]
    async fn run_turn_sends_planner_preferences_to_skill_planner() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer|systematic-debugging", "done"]);
        let mut cfg = test_cfg("project:T");
        cfg.apply_planner_preferences(
            true,
            vec!["fixer".to_string(), "oracle".to_string()],
            vec!["systematic-debugging".to_string()],
        );

        run_turn(
            &mem,
            &backend,
            &cfg,
            "fix the bug",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        let planner_prompt = backend.prompts.lock().unwrap()[0].clone();
        assert!(planner_prompt.contains("[User Preferences]"));
        assert!(planner_prompt.contains("Preferred roles: fixer, oracle"));
        assert!(planner_prompt.contains("Preferred skills: systematic-debugging"));
        assert!(planner_prompt.contains("not hard constraints"));
    }

    #[tokio::test]
    async fn run_turn_drops_invalid_preference_ids_before_planning() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle|none", "answer"]);
        let mut cfg = test_cfg("project:T");
        cfg.apply_planner_preferences(
            true,
            vec!["unknown-role".to_string()],
            vec!["unknown-skill".to_string()],
        );

        run_turn(&mem, &backend, &cfg, "think", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();

        let planner_prompt = backend.prompts.lock().unwrap()[0].clone();
        assert!(!planner_prompt.contains("[User Preferences]"));
    }

    fn test_cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: true,
        }
    }

    #[tokio::test]
    async fn run_turn_injects_planned_skill_into_execute_prompt() {
        let workspace = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", workspace.path());
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer|test-driven-development", "done"]);
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
        assert_eq!(out.role, Role::Fixer);
        assert_eq!(out.text, "done");
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("test-driven-development"));
        assert!(prompts[1].contains("[技能規範指引]"));
        assert!(prompts[1].contains("test-driven-development"));
        assert!(prompts[1].contains(".wukong/skills/superpowers/test-driven-development/SKILL.md"));
        assert!(!prompts[1].contains("Docker runtime"));
        assert!(!prompts[1].contains("crates/wukong-skills/assets/superpowers"));
        assert!(prompts[1].contains("你是 Fixer"));
        std::env::remove_var("WUKONG_WORKSPACE");
    }

    #[tokio::test]
    async fn run_turn_injects_scheduling_capability_into_final_step_only() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer, fixer", "e1", "f2"]);
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
        // prompts[0] = planner, [1] = explorer (helper), [2] = fixer (final).
        assert_eq!(prompts.len(), 3);
        assert!(!prompts[1].contains("[排程能力]"));
        assert!(prompts[2].contains("[排程能力]"));
        assert!(prompts[2].contains("schedule add-turn"));
        assert!(prompts[2].contains("--scope \"project:T\""));
    }

    #[tokio::test]
    async fn run_turn_ignores_unknown_planned_skill() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle|unknown-skill", "answer"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "think",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(!prompts[1].contains("[技能規範指引]"));
        assert!(prompts[1].contains("你是 Oracle"));
    }

    #[tokio::test]
    async fn run_turn_routes_executes_and_persists() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
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
        assert_eq!(out.role, Role::Fixer);
        assert_eq!(out.text, "done");

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
    }

    #[tokio::test]
    async fn execution_prompt_carries_role_and_not_persona() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "fix the bug",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(!prompts[1].contains("孫悟空"));
        assert!(prompts[1].contains("你是 Fixer"));
    }

    #[tokio::test]
    async fn run_turn_threads_session_into_final_step() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        let backend = MockBackend::new(&["oracle", "answer"]);
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
            assert_eq!(ids[0], None);
            assert_eq!(ids[1], Some("ses_old".to_string()));
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

    #[tokio::test]
    async fn run_turn_threads_only_final_chain_step() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        let backend = MockBackend::new(&["explorer, fixer", "e1", "f2"]);
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
        let ids = backend.session_ids.lock().unwrap();
        assert_eq!(ids.clone(), vec![None, None, Some("ses_old".to_string())]);
    }

    #[tokio::test]
    async fn unsupported_compaction_rotates_after_new_session_succeeds() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        for _ in 0..20 {
            mem.acquire_agent_session_lease("project:T", "seed", 300)
                .await
                .unwrap();
            mem.finish_agent_session_turn("project:T", "seed", Some("ses_old"))
                .await
                .unwrap();
        }
        let backend = MockBackend::with_compact_error(&["oracle", "answer"], 404)
            .with_response_session_ids(&["ses_helper", "ses_new"]);

        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "rotate",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        assert_eq!(
            mem.agent_session("project:T").await.unwrap(),
            Some("ses_new".to_string())
        );
        assert!(backend
            .deleted_sessions
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == "ses_old"));
    }

    #[tokio::test]
    async fn transient_compaction_failure_keeps_existing_session() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        for _ in 0..20 {
            mem.acquire_agent_session_lease("project:T", "seed", 300)
                .await
                .unwrap();
            mem.finish_agent_session_turn("project:T", "seed", Some("ses_old"))
                .await
                .unwrap();
        }
        let backend = MockBackend::with_compact_error(&["oracle", "answer"], 500)
            .with_response_session_ids(&["ses_helper", "ses_old"]);

        run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "retain",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();

        assert_eq!(
            mem.agent_session("project:T").await.unwrap(),
            Some("ses_old".to_string())
        );
        assert!(!backend
            .deleted_sessions
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == "ses_old"));
        assert_eq!(
            mem.agent_session_state("project:T")
                .await
                .unwrap()
                .unwrap()
                .turn_count,
            21
        );
    }

    #[tokio::test]
    async fn run_turn_runs_multi_role_chain() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer, fixer", "f1", "f2"]);
        let mut roles_seen: Vec<Role> = Vec::new();
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |r| roles_seen.push(r),
        )
        .await
        .unwrap();

        assert_eq!(roles_seen, vec![Role::Explorer, Role::Fixer]);
        assert_eq!(out.text, "f2");
        assert_eq!(out.role, Role::Fixer);

        {
            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 3);
            assert!(prompts[2].contains("f1"));
        }

        let r = mem
            .recall(RecallQuery {
                query: "build and fix".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: f2")));
        assert!(!r.data.iter().any(|h| h.text.contains("Assistant: f1")));
    }

    #[tokio::test]
    async fn run_turn_observed_reports_only_non_final_steps() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer, fixer", "e1", "f2"]);
        let mut steps: Vec<(Role, String)> = Vec::new();
        let out = run_turn_observed(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |_| {},
            &mut |role, output| steps.push((role, output.to_string())),
        )
        .await
        .unwrap();

        // Only the helper (Explorer) step is observed; the final (Fixer) output
        // is delivered via TurnOutput, not on_step.
        assert_eq!(steps, vec![(Role::Explorer, "e1".to_string())]);
        assert_eq!(out.role, Role::Fixer);
        assert_eq!(out.text, "f2");
    }

    #[tokio::test]
    async fn run_turn_observed_skips_empty_non_final_step() {
        let mem = open_memory().await;
        // Explorer (helper) returns empty; nothing should be observed for it.
        let backend = MockBackend::new(&["explorer, fixer", "", "f2"]);
        let mut steps: Vec<(Role, String)> = Vec::new();
        let out = run_turn_observed(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "go",
            &mut |_| {},
            &mut |_| {},
            &mut |role, output| steps.push((role, output.to_string())),
        )
        .await
        .unwrap();

        assert!(steps.is_empty());
        assert_eq!(out.text, "f2");
    }

    #[tokio::test]
    async fn run_turn_traced_reports_role_skill_and_output_for_helper_steps() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer|systematic-debugging\nfixer|none", "e1", "f2"]);
        let mut steps: Vec<(Role, Option<String>, String)> = Vec::new();

        let out = run_turn_traced(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |_| {},
            &mut |step| {
                steps.push((
                    step.role,
                    step.skill_name.map(str::to_string),
                    step.output.to_string(),
                ));
            },
        )
        .await
        .unwrap();

        assert_eq!(
            steps,
            vec![(
                Role::Explorer,
                Some("systematic-debugging".to_string()),
                "e1".to_string()
            )]
        );
        assert_eq!(out.role, Role::Fixer);
        assert_eq!(out.text, "f2");
    }

    #[tokio::test]
    async fn run_turn_observed_remains_role_output_compatible() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer|systematic-debugging\nfixer|none", "e1", "f2"]);
        let mut steps: Vec<(Role, String)> = Vec::new();

        run_turn_observed(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |_| {},
            &mut |role, output| steps.push((role, output.to_string())),
        )
        .await
        .unwrap();

        assert_eq!(steps, vec![(Role::Explorer, "e1".to_string())]);
    }

    #[tokio::test]
    async fn run_turn_falls_back_when_final_output_empty() {
        let mem = open_memory().await;
        // planner -> explorer,fixer; explorer reports findings; fixer (final) returns empty.
        let backend = MockBackend::new(&["explorer, fixer", "找到了根因", ""]);
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "find and fix the bug",
            &mut |_| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        assert_eq!(out.role, Role::Explorer);
        assert_eq!(out.text, "找到了根因");

        let r = mem
            .recall(RecallQuery {
                query: "find and fix the bug".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r
            .data
            .iter()
            .any(|h| h.text.contains("Assistant: 找到了根因")));
    }

    #[tokio::test]
    async fn run_turn_all_empty_repairs_with_text() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "", "直接回答原問題"]);
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
        assert_eq!(backend.prompts.lock().unwrap().len(), 3);
        let repair_prompt = backend.prompts.lock().unwrap()[2].clone();
        assert!(repair_prompt.contains("[修復回覆]"));
        assert!(repair_prompt.contains("不要呼叫工具"));
        assert!(repair_prompt.contains("你目前是什麼模型"));

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
        let backend = MockBackend::new(&["fixer", "", ""]);
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
        assert_eq!(backend.prompts.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn run_turn_injects_final_answer_directive_into_final_step_only() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["explorer, fixer", "e1", "f2"]);
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
        // prompts[0]=planner, [1]=explorer (helper), [2]=fixer (final).
        assert_eq!(prompts.len(), 3);
        assert!(!prompts[1].contains("[輸出要求]"));
        assert!(prompts[2].contains("[輸出要求]"));
    }
}
