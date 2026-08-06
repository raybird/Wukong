use crate::job::{Job, JobKind, MaintenanceTask, SchedulerError};
use crate::store::{RunStatus, SchedulerStore};
use wukong_gateway::backend::{AgentBackend, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::stream::{permission_id, QuestionRequest, PERMISSION_ALLOW_ONCE_LABEL};
use wukong_gateway::{GatewayError, StreamEvent};
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_consolidate, memory_prune, memory_snapshot};
use wukong_runtime::WukongError;

const SCHEDULED_TURN_AUTONOMY_HINT: &str = "\n\n[無人值守排程]\n這是由 scheduler 觸發的無人值守任務。不得呼叫 question 或要求使用者確認；如果流程、技能或工具規則需要使用者選擇，請直接自動採用推薦選項，並繼續完成任務。";
const QUESTION_NOTE_HEADER: &str = "[無人值守權限]";
/// 回覆詢問的嘗試次數上限（含第一次）。
const QUESTION_REPLY_ATTEMPTS: usize = 3;
const QUESTION_REPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

/// 無人值守回合遇到 opencode 詢問時的處置方式。
///
/// 上面的 prompt hint 只能約束模型主動呼叫 question 工具；permission 是
/// opencode server 在工具執行前自己攔下來的，模型再聽話也擋不住，因此一定
/// 要有這層明確策略，否則排程回合會一路等到 stream deadline 才失敗。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionPolicy {
    /// 一律拒絕（預設）。
    #[default]
    Reject,
    /// 自動允許權限請求一次；僅適用於明確信任 container 隔離的部署。
    AllowOnce,
}

impl PermissionPolicy {
    /// 只有 `WUKONG_SCHED_PERMISSION=allow` 會開啟自動允許，其餘一律拒絕。
    pub fn from_env() -> Self {
        match std::env::var("WUKONG_SCHED_PERMISSION")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("allow") => Self::AllowOnce,
            _ => Self::Reject,
        }
    }
}

/// 能處置 opencode 待決詢問的 backend。回覆必須在 turn 進行中送出，所以這裡
/// 是 async；scheduler 以它把「無人回答」變成明確的允許或拒絕。
#[allow(async_fn_in_trait)]
pub trait QuestionResponder {
    async fn allow_permission_once(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError>;

    async fn reject_question(&self, session_id: &str, request_id: &str)
        -> Result<(), GatewayError>;
}

impl QuestionResponder for AgentBackend {
    async fn allow_permission_once(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        self.answer_question(
            session_id,
            request_id,
            vec![vec![PERMISSION_ALLOW_ONCE_LABEL.to_string()]],
        )
        .await
    }

    async fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        self.cancel_question(session_id, request_id).await
    }
}

pub struct ExecutionContext<'a, B: AiBackend + QuestionResponder + Sync> {
    pub memory: &'a Memory,
    pub backend: &'a B,
    pub base_config: &'a GatewayConfig,
    /// 這批排程回合遇到權限詢問時的處置方式。
    pub permission_policy: PermissionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub success: bool,
    pub message: String,
}

pub async fn execute_job<B: AiBackend + QuestionResponder + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
) -> ExecutionOutput {
    match execute_job_inner(ctx, job).await {
        Ok(message) => ExecutionOutput {
            success: true,
            message,
        },
        Err(err) => ExecutionOutput {
            success: false,
            message: err.to_string(),
        },
    }
}

/// Outcome of running a single already-claimed job to completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimedJobOutcome {
    /// The job ran and its completion was recorded while the lease was still held.
    Completed(ExecutionOutput),
    /// Another worker took the lease before completion could be recorded. The
    /// job still executed (output attached), but this worker did not own the
    /// finish, so it must not reschedule or announce success.
    LeaseLost(ExecutionOutput),
}

/// Run one already-claimed `job` through its full lifecycle: record a run start,
/// execute it, record the run result, then release the lease and reschedule.
///
/// This is the single source of truth for the `start_run → execute → finish_run
/// → complete_claimed_job` sequence shared by the CLI (`wukong schedule run`)
/// and the daemon (`wukong-schedulerd`). It returns
/// [`ClaimedJobOutcome::LeaseLost`] when the lease was stolen before completion;
/// callers decide how to surface success, failure, and lease loss.
pub async fn run_claimed_job<B: AiBackend + QuestionResponder + Sync>(
    store: &SchedulerStore,
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
    worker_id: &str,
) -> Result<ClaimedJobOutcome, SchedulerError> {
    let started_at = chrono::Utc::now().timestamp();
    let run_id = store.start_run(&job.id, started_at).await?;
    let output = execute_job(ctx, job).await;
    let finished_at = chrono::Utc::now().timestamp();
    let status = if output.success {
        RunStatus::Success
    } else {
        RunStatus::Failure
    };
    store
        .finish_run(run_id, status, &output.message, finished_at)
        .await?;
    if store
        .complete_claimed_job(job, worker_id, finished_at)
        .await?
    {
        Ok(ClaimedJobOutcome::Completed(output))
    } else {
        Ok(ClaimedJobOutcome::LeaseLost(output))
    }
}

async fn execute_job_inner<B: AiBackend + QuestionResponder + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
) -> Result<String, WukongError> {
    match &job.kind {
        JobKind::Turn { scope, prompt } => {
            let mut cfg = ctx.base_config.clone();
            cfg.scope = scope.clone();
            let prompt = format!("{prompt}{SCHEDULED_TURN_AUTONOMY_HINT}");
            run_unattended_turn(ctx, &cfg, &prompt).await
        }
        JobKind::Maintenance { scope, task } => match task {
            MaintenanceTask::Snapshot => memory_snapshot(ctx.memory, scope.as_deref()).await,
            MaintenanceTask::Consolidate => {
                let scope = scope.as_deref().unwrap_or(&ctx.base_config.scope);
                memory_consolidate(ctx.memory, ctx.backend, scope, false).await
            }
            MaintenanceTask::Prune => memory_prune(ctx.memory, scope.as_deref(), false).await,
        },
    }
}

/// 跑一個無人值守回合，並在回合進行中即時處置 opencode 送出的詢問。
///
/// `run_turn` 的 event callback 是同步的，沒辦法在裡面 await 回覆，所以詢問先
/// 丟進 channel，再由同一個 task 的 select 迴圈回覆——這樣回覆會與 turn 併行
/// 發生，opencode 才不會一直等到 stream deadline。
async fn run_unattended_turn<B: AiBackend + QuestionResponder + Sync>(
    ctx: &ExecutionContext<'_, B>,
    cfg: &GatewayConfig,
    prompt: &str,
) -> Result<String, WukongError> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut on_event = |event: StreamEvent| {
        if let StreamEvent::QuestionRequest(request) = event {
            let _ = tx.send(request);
        }
    };
    let mut on_role = |_| {};
    let turn = wukong_runtime::run_turn(
        ctx.memory,
        ctx.backend,
        cfg,
        prompt,
        &mut on_event,
        &mut on_role,
    );
    tokio::pin!(turn);

    let mut notes: Vec<String> = Vec::new();
    let result = loop {
        tokio::select! {
            result = &mut turn => break result,
            Some(request) = rx.recv() => {
                match handle_unattended_question(ctx.backend, ctx.permission_policy, &request).await
                {
                    Ok(note) => notes.push(note),
                    // 回覆送不出去時，opencode 那邊的詢問不會自己消失，繼續等
                    // 只是換個方式等到 stream deadline。直接中止這一回合，讓
                    // 失敗在秒級發生而不是二十分鐘後。
                    Err(note) => {
                        notes.push(note);
                        break Err(WukongError::Backend(GatewayError::AgentFailed {
                            code: None,
                            stderr: "無法回覆 opencode 詢問，已中止本回合".to_string(),
                        }));
                    }
                }
            }
        }
    };

    match result {
        Ok(out) => Ok(append_question_notes(out.text, &notes)),
        // 失敗時也要把處置結果帶出去，否則 run 只會留下一句 backend error，
        // 看不出這回合其實碰到了權限問題。
        Err(err) if !notes.is_empty() => Err(WukongError::Backend(GatewayError::AgentFailed {
            code: None,
            stderr: format!("{err}\n{QUESTION_NOTE_HEADER}\n{}", notes.join("\n")),
        })),
        Err(err) => Err(err),
    }
}

/// 依策略處置一則詢問。`Ok` 表示已送達 opencode，`Err` 表示重試後仍送不出去，
/// 兩者都回傳要寫進 run 訊息的一行摘要。
///
/// 會重試是因為事故時間線裡出現過 opencode server 輪替造成的暫時性解析／連線
/// 失敗；重試次數有上限，超過就交給呼叫端中止，不會演變成另一種無限等待。
async fn handle_unattended_question<B: QuestionResponder>(
    backend: &B,
    policy: PermissionPolicy,
    request: &QuestionRequest,
) -> Result<String, String> {
    let summary = question_summary(request);
    let request_id = &request.request_id;
    // 只有 permission 請求能自動允許；一般 question 在無人值守情境一律拒絕。
    let allow = permission_id(request_id).is_some() && policy == PermissionPolicy::AllowOnce;
    let action = if allow {
        "已自動允許"
    } else {
        "已自動拒絕"
    };

    let mut last_error = None;
    for attempt in 1..=QUESTION_REPLY_ATTEMPTS {
        let outcome = if allow {
            backend
                .allow_permission_once(&request.session_id, request_id)
                .await
        } else {
            backend
                .reject_question(&request.session_id, request_id)
                .await
        };
        match outcome {
            Ok(()) => {
                eprintln!(
                    "scheduler_question_handled request_id={request_id} session_id={} action={action} attempt={attempt} summary={summary}",
                    request.session_id
                );
                return Ok(format!("{action} {request_id}：{summary}"));
            }
            Err(err) => {
                eprintln!(
                    "scheduler_question_reply_failed request_id={request_id} session_id={} action={action} attempt={attempt} error={err}",
                    request.session_id
                );
                last_error = Some(err);
                if attempt < QUESTION_REPLY_ATTEMPTS {
                    tokio::time::sleep(QUESTION_REPLY_RETRY_DELAY).await;
                }
            }
        }
    }

    let detail = last_error
        .map(|err| err.to_string())
        .unwrap_or_else(|| "未知錯誤".to_string());
    Err(format!(
        "處置失敗（{action}）{request_id}：{detail}（已重試 {QUESTION_REPLY_ATTEMPTS} 次）"
    ))
}

/// 把詢問內容壓成單行摘要，權限請求的類型與路徑才進得了 run 訊息。
fn question_summary(request: &QuestionRequest) -> String {
    request
        .questions
        .first()
        .map(|question| {
            question
                .question
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| "(無詢問內容)".to_string())
}

fn append_question_notes(text: String, notes: &[String]) -> String {
    if notes.is_empty() {
        return text;
    }
    let mut out = text;
    if !out.trim().is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(QUESTION_NOTE_HEADER);
    out.push('\n');
    out.push_str(&notes.join("\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobKind, MaintenanceTask};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::stream::QuestionInfo;
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<Result<String, GatewayError>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new(replies: Vec<Result<&str, GatewayError>>) -> Self {
            Self {
                replies: Mutex::new(replies.into_iter().map(|r| r.map(str::to_string)).collect()),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))?;
            Ok(AgentResponse {
                text: reply,
                session_id: Some("ses_new".to_string()),
            })
        }
    }

    impl QuestionResponder for MockBackend {
        async fn allow_permission_once(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            unreachable!("這個 backend 不會送出詢問")
        }

        async fn reject_question(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            unreachable!("這個 backend 不會送出詢問")
        }
    }

    /// 模擬 opencode：先送出一則詢問，並且在收到回覆之前不繼續這一棒——
    /// 沒有回覆時真實情況會一路卡到 stream deadline。
    struct QuestioningBackend {
        request: QuestionRequest,
        asked: Mutex<bool>,
        allowed: Mutex<Vec<String>>,
        rejected: Mutex<Vec<String>>,
    }

    impl QuestioningBackend {
        fn new(request_id: &str, question: &str) -> Self {
            Self {
                request: QuestionRequest {
                    request_id: request_id.to_string(),
                    session_id: "ses_1".to_string(),
                    questions: vec![QuestionInfo {
                        question: question.to_string(),
                        header: "🔐 權限確認".to_string(),
                        options: Vec::new(),
                        multiple: false,
                        custom: false,
                    }],
                },
                asked: Mutex::new(false),
                allowed: Mutex::new(Vec::new()),
                rejected: Mutex::new(Vec::new()),
            }
        }

        fn handled(&self) -> usize {
            self.allowed.lock().unwrap().len() + self.rejected.lock().unwrap().len()
        }
    }

    impl AiBackend for QuestioningBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "完成".to_string(),
                session_id: Some("ses_1".to_string()),
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            let first = {
                let mut asked = self.asked.lock().unwrap();
                let first = !*asked;
                *asked = true;
                first
            };
            if first {
                on_event(StreamEvent::QuestionRequest(self.request.clone()));
                // 等到詢問被處置為止。設上限是為了讓回歸變成測試失敗，而不是
                // 讓測試整個掛住。
                for _ in 0..1000 {
                    if self.handled() > 0 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                if self.handled() == 0 {
                    return Err(GatewayError::AgentFailed {
                        code: None,
                        stderr: "詢問未被處置".to_string(),
                    });
                }
            }
            self.run(req).await
        }
    }

    impl QuestionResponder for QuestioningBackend {
        async fn allow_permission_once(
            &self,
            _session_id: &str,
            request_id: &str,
        ) -> Result<(), GatewayError> {
            self.allowed.lock().unwrap().push(request_id.to_string());
            Ok(())
        }

        async fn reject_question(
            &self,
            _session_id: &str,
            request_id: &str,
        ) -> Result<(), GatewayError> {
            self.rejected.lock().unwrap().push(request_id.to_string());
            Ok(())
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    fn job(kind: JobKind) -> Job {
        Job {
            id: "job-1".to_string(),
            name: "job".to_string(),
            kind,
            cron: "* * * * *".to_string(),
            enabled: true,
            next_run_at: Some(1),
            last_run_at: None,
        }
    }

    /// 模擬「詢問送出後 opencode 永遠不會自己前進」：只有成功回覆才能讓這一棒
    /// 結束，因此回覆失敗時唯一的出路是中止回合。
    struct StuckBackend {
        request: QuestionRequest,
        asked: Mutex<bool>,
        attempts: Mutex<usize>,
    }

    impl StuckBackend {
        fn new(request_id: &str) -> Self {
            Self {
                request: QuestionRequest {
                    request_id: request_id.to_string(),
                    session_id: "ses_1".to_string(),
                    questions: vec![QuestionInfo {
                        question: "OpenCode 要求執行權限：external_directory".to_string(),
                        header: "🔐 權限確認".to_string(),
                        options: Vec::new(),
                        multiple: false,
                        custom: false,
                    }],
                },
                asked: Mutex::new(false),
                attempts: Mutex::new(0),
            }
        }
    }

    impl AiBackend for StuckBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "完成".to_string(),
                session_id: Some("ses_1".to_string()),
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            let first = {
                let mut asked = self.asked.lock().unwrap();
                let first = !*asked;
                *asked = true;
                first
            };
            if first {
                on_event(StreamEvent::QuestionRequest(self.request.clone()));
                // 真實情況會卡在這裡直到 WUKONG_AGENT_TIMEOUT_SECS。
                std::future::pending::<()>().await;
            }
            self.run(req).await
        }
    }

    impl QuestionResponder for StuckBackend {
        async fn allow_permission_once(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            *self.attempts.lock().unwrap() += 1;
            Err(GatewayError::AgentFailed {
                code: None,
                stderr: "permission_reply 連線失敗".to_string(),
            })
        }

        async fn reject_question(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            *self.attempts.lock().unwrap() += 1;
            Err(GatewayError::AgentFailed {
                code: None,
                stderr: "permission_reply 連線失敗".to_string(),
            })
        }
    }

    /// 回覆送不出去時不能默默繼續等：那只是把 1200 秒的等待換個位置發生。
    #[tokio::test]
    async fn unattended_turn_aborts_when_question_reply_keeps_failing() {
        let memory = open_memory().await;
        let backend = StuckBackend::new("permission-per_1");
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        // 逾時代表回歸：回合又變成等 backend 而不是自己收尾。
        let out = tokio::time::timeout(std::time::Duration::from_secs(5), execute_job(&ctx, &job))
            .await
            .expect("回覆失敗時應立刻中止回合，而不是等到 agent timeout");

        assert!(!out.success);
        assert!(out.message.contains("已中止本回合"), "{}", out.message);
        assert!(out.message.contains("處置失敗"), "{}", out.message);
        assert!(out.message.contains("permission-per_1"), "{}", out.message);
        // 暫時性失敗要重試，但次數有上限。
        assert_eq!(*backend.attempts.lock().unwrap(), QUESTION_REPLY_ATTEMPTS);
    }

    /// 這是本次事故的核心回歸：排程回合收到權限詢問時必須當場處置。過去
    /// callback 是 no-op，opencode 會一直等，直到 1200 秒 deadline 才失敗。
    #[tokio::test]
    async fn unattended_turn_rejects_permission_by_default() {
        let memory = open_memory().await;
        let backend = QuestioningBackend::new(
            "permission-per_1",
            "OpenCode 要求執行權限：external_directory\n\n範圍：\n• /tmp/*",
        );
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success, "{}", out.message);
        assert_eq!(
            backend.rejected.lock().unwrap().as_slice(),
            ["permission-per_1"]
        );
        assert!(backend.allowed.lock().unwrap().is_empty());
        // 處置結果要留在 run 訊息裡，否則看 scheduler_runs 不會知道被擋了什麼。
        assert!(
            out.message.contains(QUESTION_NOTE_HEADER),
            "{}",
            out.message
        );
        assert!(out.message.contains("已自動拒絕"), "{}", out.message);
        assert!(
            out.message.contains("external_directory"),
            "{}",
            out.message
        );
        assert!(out.message.contains("/tmp/*"), "{}", out.message);
    }

    #[tokio::test]
    async fn unattended_turn_can_auto_allow_permission_requests() {
        let memory = open_memory().await;
        let backend = QuestioningBackend::new("permission-per_1", "OpenCode 要求執行權限：bash");
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::AllowOnce,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success, "{}", out.message);
        assert_eq!(
            backend.allowed.lock().unwrap().as_slice(),
            ["permission-per_1"]
        );
        assert!(backend.rejected.lock().unwrap().is_empty());
        assert!(out.message.contains("已自動允許"), "{}", out.message);
    }

    /// auto-allow 只針對 permission；一般 question 在無人值守情境仍要拒絕，
    /// 否則等於讓排程工作替使用者亂選選項。
    #[tokio::test]
    async fn auto_allow_does_not_answer_plain_questions() {
        let memory = open_memory().await;
        let backend = QuestioningBackend::new("que_1", "要用哪一個方案？");
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::AllowOnce,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success, "{}", out.message);
        assert_eq!(backend.rejected.lock().unwrap().as_slice(), ["que_1"]);
        assert!(backend.allowed.lock().unwrap().is_empty());
    }

    #[test]
    fn permission_policy_defaults_to_reject() {
        assert_eq!(PermissionPolicy::default(), PermissionPolicy::Reject);
    }

    #[tokio::test]
    async fn turn_job_overrides_scope() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("done")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert_eq!(out.message, "done");
        assert!(memory
            .agent_session("project:Scheduled")
            .await
            .unwrap()
            .is_some());
        assert_eq!(memory.agent_session("project:Base").await.unwrap(), None);
    }

    #[tokio::test]
    async fn turn_job_returns_failure_message_when_backend_fails() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Err(GatewayError::AgentFailed {
            code: Some(1),
            stderr: "boom".to_string(),
        })]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(!out.success);
        assert!(out.message.contains("boom"));
    }

    #[tokio::test]
    async fn turn_job_instructs_agent_to_auto_choose_recommended_options() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("done")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        let prompts = backend.prompts.lock().unwrap();
        assert!(prompts.iter().any(|p| p.contains("無人值守排程")));
        assert!(prompts.iter().any(|p| p.contains("自動採用推薦選項")));
    }

    #[tokio::test]
    async fn snapshot_maintenance_returns_text() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Maintenance {
            scope: None,
            task: MaintenanceTask::Snapshot,
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert!(out.message.contains("總計:"));
    }

    #[tokio::test]
    async fn prune_maintenance_uses_provided_scope() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = job(JobKind::Maintenance {
            scope: Some("project:Scheduled".to_string()),
            task: MaintenanceTask::Prune,
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert!(out.message.contains("已刪除"));
    }
}
