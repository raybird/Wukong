use crate::job::{Job, JobKind, MaintenanceTask, SchedulerError};
use crate::store::{RunStatus, SchedulerStore};
use wukong_gateway::backend::{AgentBackend, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::stream::{permission_id, QuestionRequest, PERMISSION_ALLOW_ONCE_LABEL};
use wukong_gateway::{GatewayError, StreamEvent};
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_consolidate, memory_prune, memory_snapshot};
use wukong_runtime::{Role, WukongError};

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
    execute_job_observed(ctx, job, &mut |_| {}).await
}

/// 同 [`execute_job`]，另外在 Turn 回合每一棒開始時呼叫 `on_role`。
async fn execute_job_observed<B: AiBackend + QuestionResponder + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
    on_role: &mut dyn FnMut(Role),
) -> ExecutionOutput {
    match execute_job_inner(ctx, job, on_role).await {
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
    /// 2026-10-21（issue-0017）：回合跑完時排程已經被刪除（例如回合自己呼叫了
    /// `schedule rm`）。scope 已清掉；結果照常回報與送出通知。
    Deleted(ExecutionOutput),
    /// 2026-10-21（issue-0013）：要開始執行時 lease 已經不在這個 worker 手上。同一批
    /// 取得的 job 排隊期間可能被別人取得執行，也可能被停用、重新啟用或刪除（都會清掉
    /// lease）。沒有執行，也沒有 run 紀錄。
    NotHeld,
}

/// 2026-10-21（issue-0017）：刪除排程的結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteJobOutcome {
    NotFound,
    /// 刪除時沒有人持有它，已經清掉它的 scope。
    Deleted,
    /// 刪除時正在執行，或已被取得、還在排隊。scope 等 executor 處理完這個 job 再清。
    DeletedWhileHeld,
}

/// 刪除排程，並清掉 Turn 排程的 scope（`agent:schedule-<id>`）的記憶與 session 指向。
/// CLI 的 `schedule rm` 與 Web 的刪除共用。執行中的回合在結束時才寫入記憶、重建 session
/// 狀態，提早清也會被寫回去，所以被持有的排程只刪 job，scope 由 `run_claimed_job` 事後清理。
pub async fn delete_job(
    store: &SchedulerStore,
    memory: &Memory,
    id: &str,
) -> Result<DeleteJobOutcome, SchedulerError> {
    Ok(match store.remove_job(id).await? {
        None => DeleteJobOutcome::NotFound,
        Some(true) => DeleteJobOutcome::DeletedWhileHeld,
        Some(false) => {
            memory.delete_scope(&scheduled_turn_scope(id)).await?;
            DeleteJobOutcome::Deleted
        }
    })
}

/// 排程已經被刪除時，清掉它的 scope 並回 `true`（issue-0017）。清理失敗只記 log：
/// 呼叫端此時手上是已經跑完或已經跳過的 job，結果仍要照常回報。
async fn clear_scope_if_deleted(
    store: &SchedulerStore,
    memory: &Memory,
    job_id: &str,
) -> Result<bool, SchedulerError> {
    if store.get_job(job_id).await?.is_some() {
        return Ok(false);
    }
    if let Err(err) = memory.delete_scope(&scheduled_turn_scope(job_id)).await {
        eprintln!("scheduler_scope_cleanup_failed job={job_id}: {err}");
    }
    Ok(true)
}

/// 2026-10-21（issue-0022）：清掉排程已不存在的 Turn 排程 scope，給 daemon 啟動時用。
/// 只處理後綴是排程 id 寫法（小寫、有連字號的 UUID）的 `agent:schedule-<id>`；其他寫法即使能
/// 解析成 UUID，`get_job` 也一定查不到，不能當成孤兒。先確認所有候選的排程都查得到
/// 結果，再開始刪，查詢出錯就整個回錯誤、不刪任何東西。回傳清掉的 scope 數。
pub async fn clear_orphan_turn_scopes(
    store: &SchedulerStore,
    memory: &Memory,
) -> Result<u64, SchedulerError> {
    let mut orphans = Vec::new();
    for scope in memory.scopes_with_prefix(TURN_SCOPE_PREFIX).await? {
        let id = &scope[TURN_SCOPE_PREFIX.len()..];
        let is_job_id = uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id);
        if is_job_id && store.get_job(id).await?.is_none() {
            orphans.push(scope);
        }
    }
    for scope in &orphans {
        memory.delete_scope(scope).await?;
    }
    Ok(orphans.len() as u64)
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
    lease_secs: i64,
) -> Result<ClaimedJobOutcome, SchedulerError> {
    let started_at = chrono::Utc::now().timestamp();
    // 2026-10-21（issue-0013）：同一批取得的 job 排隊時只有正在跑的那個會續租。輪到這個
    // job 時先續租一次；lease 已經不在自己手上，代表它在等待期間被別人取得執行，或被停用、
    // 重新啟用、刪除。
    if !store
        .renew_job_lease(&job.id, worker_id, started_at, lease_secs)
        .await?
    {
        clear_scope_if_deleted(store, ctx.memory, &job.id).await?;
        return Ok(ClaimedJobOutcome::NotHeld);
    }
    let run_id = store.start_run(&job.id, started_at).await?;
    // 2026-10-21（issue-0022）：回合開始後就推進，行程中途結束時不重跑這一次，避免重複送出訊息
    // 或寫入檔案。維護排程補跑不會重複對外的動作，不推進，中斷後 lease 到期就補跑。
    if matches!(job.kind, JobKind::Turn { .. }) {
        store.advance_next_run(job, worker_id, started_at).await?;
    }
    let output =
        execute_with_heartbeat(store, ctx, job, worker_id, lease_secs, run_id, started_at).await;
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
    } else if clear_scope_if_deleted(store, ctx.memory, &job.id).await? {
        Ok(ClaimedJobOutcome::Deleted(output))
    } else {
        Ok(ClaimedJobOutcome::LeaseLost(output))
    }
}

/// 排程 run 目前的階段（issue-0013），寫進執行中的 run 訊息，以及失敗訊息的開頭。
enum Phase {
    Maintenance,
    Planning,
    Step(usize, Role),
}

impl Phase {
    fn initial(job: &Job) -> Self {
        match job.kind {
            JobKind::Turn { .. } => Phase::Planning,
            JobKind::Maintenance { .. } => Phase::Maintenance,
        }
    }

    fn advance(&mut self, role: Role) {
        let next = match self {
            Phase::Step(k, _) => *k + 1,
            _ => 1,
        };
        *self = Phase::Step(next, role);
    }

    fn label(&self) -> String {
        match self {
            Phase::Maintenance => "維護作業".to_string(),
            Phase::Planning => "規劃中".to_string(),
            Phase::Step(k, role) => format!("第 {k} 棒 {}", role.name()),
        }
    }

    /// 角色名稱是英文，後面接「失敗」要隔一個空白。
    fn failed(&self) -> String {
        match self {
            Phase::Step(..) => format!("{} 失敗", self.label()),
            _ => format!("{}失敗", self.label()),
        }
    }
}

/// 進度與 job lease 的更新間隔：lease 的三分之一，最長 30 秒。
fn heartbeat_every(lease_secs: i64) -> std::time::Duration {
    std::time::Duration::from_millis((lease_secs.max(1) as u64 * 1000 / 3).min(30_000))
}

/// 2026-10-21（issue-0013）：執行 job，期間定期續租 job lease，並把目前階段與已執行
/// 秒數寫進 run 訊息；換階段時立刻寫。續租與執行在同一個 task，行程中止就不再續租。
/// Turn 回合失敗時，訊息開頭加上失敗的階段。
async fn execute_with_heartbeat<B: AiBackend + QuestionResponder + Sync>(
    store: &SchedulerStore,
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
    worker_id: &str,
    lease_secs: i64,
    run_id: i64,
    started_at: i64,
) -> ExecutionOutput {
    let elapsed = || chrono::Utc::now().timestamp() - started_at;
    let (tx, mut roles) = tokio::sync::mpsc::unbounded_channel();
    let mut on_role = move |role| {
        let _ = tx.send(role);
    };
    let work = execute_job_observed(ctx, job, &mut on_role);
    tokio::pin!(work);

    let mut phase = Phase::initial(job);
    let progress = |phase: &Phase| format!("進行中：{}，已執行 {} 秒", phase.label(), elapsed());
    let report = |message: String| async move {
        if let Err(err) = store.update_running_message(run_id, &message).await {
            eprintln!("scheduler_progress_write_failed run={run_id}: {err}");
        }
    };
    report(progress(&phase)).await;

    let every = heartbeat_every(lease_secs);
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut output = loop {
        tokio::select! {
            output = &mut work => break output,
            Some(role) = roles.recv() => {
                phase.advance(role);
                report(progress(&phase)).await;
            }
            _ = ticker.tick() => {
                let now = chrono::Utc::now().timestamp();
                match store.renew_job_lease(&job.id, worker_id, now, lease_secs).await {
                    Ok(true) => {}
                    Ok(false) => eprintln!("scheduler_lease_renew_lost job={} worker={worker_id}", job.id),
                    Err(err) => eprintln!("scheduler_lease_renew_failed job={}: {err}", job.id),
                }
                report(progress(&phase)).await;
            }
        }
    };
    // 最後一棒可能剛開始就失敗，那次 on_role 還留在 channel 裡。
    while let Ok(role) = roles.try_recv() {
        phase.advance(role);
    }
    if !output.success && !matches!(phase, Phase::Maintenance) {
        output.message = format!(
            "{}，已執行 {} 秒：{}",
            phase.failed(),
            elapsed(),
            output.message
        );
    }
    output
}

async fn execute_job_inner<B: AiBackend + QuestionResponder + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
    on_role: &mut dyn FnMut(Role),
) -> Result<String, WukongError> {
    match &job.kind {
        // job 的 scope 是擁有者，回送與聊天紀錄用它；回合本身在排程自己的 scope 執行。
        JobKind::Turn { prompt, .. } => {
            let mut cfg = ctx.base_config.clone();
            cfg.scope = scheduled_turn_scope(&job.id);
            let prompt = format!("{prompt}{SCHEDULED_TURN_AUTONOMY_HINT}");
            run_unattended_turn(ctx, &cfg, &prompt, on_role).await
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

const TURN_SCOPE_PREFIX: &str = "agent:schedule-";

/// 2026-10-20（issue-0014）：排程回合的記憶、召回與 session 都落在這個 scope，
/// 不寫進建立排程的使用者 scope；同一個排程的多次觸發共用它。
fn scheduled_turn_scope(job_id: &str) -> String {
    format!("{TURN_SCOPE_PREFIX}{job_id}")
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
    on_role: &mut dyn FnMut(Role),
) -> Result<String, WukongError> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut on_event = |event: StreamEvent| {
        if let StreamEvent::QuestionRequest(request) = event {
            let _ = tx.send(request);
        }
    };
    let turn =
        wukong_runtime::run_turn(ctx.memory, ctx.backend, cfg, prompt, &mut on_event, on_role);
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
    use wukong_memory::{MemoryItem, MemoryKind, RememberInput};

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

    /// 每次呼叫都先睡 `delay` 再照劇本回覆，模擬跑得比 lease 久的回合。
    struct SlowBackend {
        delay: std::time::Duration,
        replies: Mutex<VecDeque<Result<String, GatewayError>>>,
    }

    impl SlowBackend {
        fn new(delay_ms: u64, replies: Vec<Result<&str, GatewayError>>) -> Self {
            Self {
                delay: std::time::Duration::from_millis(delay_ms),
                replies: Mutex::new(replies.into_iter().map(|r| r.map(str::to_string)).collect()),
            }
        }
    }

    impl AiBackend for SlowBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            tokio::time::sleep(self.delay).await;
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))?;
            Ok(AgentResponse {
                text: reply,
                session_id: Some("ses_slow".to_string()),
            })
        }
    }

    impl QuestionResponder for SlowBackend {
        async fn allow_permission_once(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            unreachable!("這個 backend 不會送出詢問")
        }

        async fn reject_question(&self, _: &str, _: &str) -> Result<(), GatewayError> {
            unreachable!("這個 backend 不會送出詢問")
        }
    }

    fn timed_out() -> GatewayError {
        GatewayError::AgentFailed {
            code: None,
            stderr: "本機 OpenCode 回合逾時，已中止程序。".to_string(),
        }
    }

    async fn open_store() -> SchedulerStore {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        SchedulerStore::open(&url).await.unwrap()
    }

    /// 新增一個 Turn 排程，並以 worker-a 取得 `lease_secs` 秒的 job lease。
    async fn claimed_turn(store: &SchedulerStore, lease_secs: i64) -> Job {
        let job = store
            .add_job(crate::job::NewJob {
                name: "long".to_string(),
                kind: JobKind::Turn {
                    scope: "user:tg-42".to_string(),
                    prompt: "長任務".to_string(),
                },
                cron: "0 9 * * *".to_string(),
            })
            .await
            .unwrap();
        store
            .claim_job(
                &job.id,
                chrono::Utc::now().timestamp(),
                "worker-a",
                lease_secs,
            )
            .await
            .unwrap()
            .unwrap()
    }

    async fn only_run(store: &SchedulerStore, job: &Job) -> crate::store::JobRun {
        let mut runs = store.recent_runs(Some(&job.id), 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        runs.remove(0)
    }

    fn elapsed_secs(message: &str) -> i64 {
        let rest = message
            .split("已執行 ")
            .nth(1)
            .unwrap_or_else(|| panic!("訊息沒有已執行秒數：{message}"));
        rest.split(' ').next().unwrap().parse().unwrap()
    }

    // SCN-004：回合跑得比 job lease 久，執行中別的 worker 拿不到 job；原回合正常完成。
    #[tokio::test]
    async fn turn_longer_than_job_lease_keeps_the_job_claimed() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = SlowBackend::new(1600, vec![Ok("oracle"), Ok("完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 2).await;

        let intruder = async {
            tokio::time::sleep(std::time::Duration::from_millis(2600)).await;
            store
                .claim_job(&job.id, chrono::Utc::now().timestamp(), "worker-b", 300)
                .await
                .unwrap()
        };
        let (outcome, stolen) =
            tokio::join!(run_claimed_job(&store, &ctx, &job, "worker-a", 2), intruder);

        assert!(stolen.is_none(), "回合執行中 job 被 worker-b 取得");
        match outcome.unwrap() {
            ClaimedJobOutcome::Completed(out) => assert!(out.success, "{}", out.message),
            other => panic!("原回合應正常完成：{other:?}"),
        }
        assert!(
            store
                .get_job(&job.id)
                .await
                .unwrap()
                .unwrap()
                .next_run_at
                .unwrap()
                > 1
        );
    }

    /// 在 `scope` 寫一筆記憶與 session 指向。
    async fn seed_scope(memory: &Memory, scope: &str) {
        memory
            .remember(wukong_memory::RememberInput {
                scope: scope.to_string(),
                session_id: None,
                items: vec![wukong_memory::MemoryItem {
                    kind: MemoryKind::Summary,
                    text: format!("{scope} 的摘要"),
                    importance: None,
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
        memory
            .set_agent_session(scope, &format!("ses-{scope}"))
            .await
            .unwrap();
    }

    /// `scope` 剩下的記憶筆數與 session 指向。
    async fn scope_left(memory: &Memory, scope: &str) -> (usize, Option<String>) {
        let records = memory
            .records(Some(scope), None, 100)
            .await
            .unwrap()
            .records;
        (records.len(), memory.agent_session(scope).await.unwrap())
    }

    const YEARLY: &str = "0 0 1 1 *";

    async fn open_store_with_url() -> (SchedulerStore, String) {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        (SchedulerStore::open(&url).await.unwrap(), url)
    }

    /// 新增一個每年一次的 Turn 排程，並把 `next_run_at` 改到 5 秒前，讓它以實際時間到期。
    async fn due_yearly_turn(store: &SchedulerStore, url: &str) -> Job {
        due_yearly_job(
            store,
            url,
            JobKind::Turn {
                scope: "user:tg-42".to_string(),
                prompt: "年度提醒".to_string(),
            },
        )
        .await
    }

    async fn due_yearly_job(store: &SchedulerStore, url: &str, kind: JobKind) -> Job {
        let job = store
            .add_job(crate::job::NewJob {
                name: "yearly".to_string(),
                kind,
                cron: YEARLY.to_string(),
            })
            .await
            .unwrap();
        let pool = sqlx::SqlitePool::connect(url).await.unwrap();
        sqlx::query("UPDATE scheduler_jobs SET next_run_at = ?1 WHERE id = ?2")
            .bind(chrono::Utc::now().timestamp() - 5)
            .bind(&job.id)
            .execute(&pool)
            .await
            .unwrap();
        store.get_job(&job.id).await.unwrap().unwrap()
    }

    // issue-0022 SCN-001：回合開始之後行程結束，lease 到期後同一次不再被取得。
    #[tokio::test]
    async fn a_turn_interrupted_after_it_started_is_not_run_again() {
        let memory = open_memory().await;
        let (store, url) = open_store_with_url().await;
        let job = due_yearly_turn(&store, &url).await;
        let started = chrono::Utc::now().timestamp();
        let claimed = store
            .claim_due_jobs(started, "worker-a", 5, 10)
            .await
            .unwrap();
        assert_eq!(claimed, vec![job.clone()]);
        let backend = SlowBackend::new(1000, vec![Ok("oracle"), Ok("不該完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };

        // 行程在回合中途結束：future 被丟掉，不會 finish_run，也不再續租。
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(300),
            run_claimed_job(&store, &ctx, &claimed[0], "worker-a", 5),
        )
        .await
        .is_err());

        let after_lease = chrono::Utc::now().timestamp() + 6;
        assert!(store
            .claim_due_jobs(after_lease, "worker-b", 300, 10)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            store.get_job(&job.id).await.unwrap().unwrap().next_run_at,
            crate::job::next_after(YEARLY, started).unwrap()
        );
        assert_eq!(store.interrupt_stale_runs(after_lease).await.unwrap(), 1);
        assert_eq!(only_run(&store, &job).await.status, RunStatus::Interrupted);
    }

    // issue-0022：維護排程中途結束時不推進，lease 到期後照常補跑；跳過只適用於 Turn 排程。
    #[tokio::test]
    async fn an_interrupted_maintenance_job_is_claimed_again() {
        let file = NamedTempFile::new().unwrap();
        let memory_url = format!("sqlite://{}", file.path().display());
        let memory = Memory::open(&memory_url).await.unwrap();
        let (store, url) = open_store_with_url().await;
        let job = due_yearly_job(
            &store,
            &url,
            JobKind::Maintenance {
                scope: Some("project:T".to_string()),
                task: MaintenanceTask::Prune,
            },
        )
        .await;
        // 讓一筆記憶符合 prune 條件，再由另一條連線拿住寫入鎖，prune 刪除時就會卡住。
        seed_scope(&memory, "project:T").await;
        let pool = sqlx::SqlitePool::connect(&memory_url).await.unwrap();
        sqlx::query("UPDATE memories SET kind = 'note', created_at = 0, importance = 0.1")
            .execute(&pool)
            .await
            .unwrap();
        let mut lock = pool.acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *lock)
            .await
            .unwrap();
        let started = chrono::Utc::now().timestamp();
        let claimed = store
            .claim_due_jobs(started, "worker-a", 5, 10)
            .await
            .unwrap();
        let backend = MockBackend::new(vec![]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };

        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(300),
            run_claimed_job(&store, &ctx, &claimed[0], "worker-a", 5),
        )
        .await
        .is_err());

        assert_eq!(only_run(&store, &job).await.status, RunStatus::Running);
        let again = store
            .claim_due_jobs(started + 6, "worker-b", 300, 10)
            .await
            .unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].next_run_at, job.next_run_at);
    }

    // issue-0022 SCN-002：取得了但還沒開始執行，lease 到期後照常被取得。
    #[tokio::test]
    async fn a_claimed_turn_that_never_started_is_claimed_again() {
        let (store, url) = open_store_with_url().await;
        let job = due_yearly_turn(&store, &url).await;
        let now = chrono::Utc::now().timestamp();
        assert_eq!(
            store
                .claim_due_jobs(now, "worker-a", 5, 10)
                .await
                .unwrap()
                .len(),
            1
        );

        // worker-a 在輪到它之前就結束了。
        let claimed = store
            .claim_due_jobs(now + 6, "worker-b", 300, 10)
            .await
            .unwrap();

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].next_run_at, job.next_run_at);
    }

    // issue-0022 SCN-003：正常完成後，下次執行時間和變更前一樣，以完成時間推算。
    #[tokio::test]
    async fn a_completed_turn_is_rescheduled_from_its_finish_time() {
        let memory = open_memory().await;
        let (store, url) = open_store_with_url().await;
        let job = due_yearly_turn(&store, &url).await;
        let claimed = store
            .claim_due_jobs(chrono::Utc::now().timestamp(), "worker-a", 300, 10)
            .await
            .unwrap();
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("年度提醒完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };

        let outcome = run_claimed_job(&store, &ctx, &claimed[0], "worker-a", 300)
            .await
            .unwrap();
        let finished = chrono::Utc::now().timestamp();

        assert!(matches!(outcome, ClaimedJobOutcome::Completed(_)));
        assert_eq!(
            store.get_job(&job.id).await.unwrap().unwrap().next_run_at,
            crate::job::next_after(YEARLY, finished).unwrap()
        );
    }

    // issue-0022 SCN-003：手動 trigger 還沒到期的排程，中途結束也不改它的下次執行時間。
    #[tokio::test]
    async fn manually_triggering_a_job_that_is_not_due_keeps_its_next_run() {
        let memory = open_memory().await;
        let store = open_store().await;
        let job = claimed_turn(&store, 5).await;
        let backend = SlowBackend::new(1000, vec![Ok("oracle"), Ok("不該完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };

        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(300),
            run_claimed_job(&store, &ctx, &job, "worker-a", 5),
        )
        .await
        .is_err());

        assert_eq!(
            store.get_job(&job.id).await.unwrap().unwrap().next_run_at,
            job.next_run_at
        );
    }

    // issue-0022 SCN-005：只清排程不存在、而且後綴是 UUID 的排程 scope。
    #[tokio::test]
    async fn clearing_orphan_scopes_keeps_live_and_non_uuid_scopes() {
        let memory = open_memory().await;
        let store = open_store().await;
        let live = claimed_turn(&store, 300).await;
        let gone = "0b5c3a52-6f2e-4c55-9d0e-0a6f1e2b3c4d";
        let orphan = scheduled_turn_scope(gone);
        // 排程 id 一律是小寫、有連字號的 UUID；其他寫法即使能解析成 UUID，也不是排程寫出來的。
        let kept = [
            scheduled_turn_scope(&live.id),
            scheduled_turn_scope(&live.id.to_uppercase()),
            scheduled_turn_scope(&format!("{{{}}}", live.id)),
            scheduled_turn_scope(&format!("urn:uuid:{}", live.id)),
            scheduled_turn_scope(&live.id.replace('-', "")),
            format!("{orphan}-x"),
            "agent:schedule-notes".to_string(),
            "user:tg-42".to_string(),
        ];
        for scope in std::iter::once(&orphan).chain(&kept) {
            seed_scope(&memory, scope).await;
        }

        assert_eq!(clear_orphan_turn_scopes(&store, &memory).await.unwrap(), 1);

        assert_eq!(scope_left(&memory, &orphan).await, (0, None));
        for scope in &kept {
            assert_eq!(
                scope_left(&memory, scope).await,
                (1, Some(format!("ses-{scope}"))),
                "{scope}"
            );
        }
        // 已經清乾淨，再跑一次不會再清任何東西。
        assert_eq!(clear_orphan_turn_scopes(&store, &memory).await.unwrap(), 0);
    }

    // issue-0022 SCN-005：查不到排程是因為出錯時，不能當成排程不存在；任何一個排程查詢出錯，
    // 排在前面、已經確認是孤兒的 scope 也不刪。
    #[tokio::test]
    async fn clearing_orphan_scopes_deletes_nothing_when_a_job_cannot_be_read() {
        let memory = open_memory().await;
        let (store, url) = open_store_with_url().await;
        let unreadable = claimed_turn(&store, 0).await;
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query("UPDATE scheduler_jobs SET kind = 'maintenance' WHERE id = ?1")
            .bind(&unreadable.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(store.get_job(&unreadable.id).await.is_err());
        let scopes = [
            scheduled_turn_scope("00000000-0000-4000-8000-000000000000"),
            scheduled_turn_scope(&unreadable.id),
        ];
        for scope in &scopes {
            seed_scope(&memory, scope).await;
        }

        assert!(clear_orphan_turn_scopes(&store, &memory).await.is_err());

        for scope in &scopes {
            assert_eq!(
                scope_left(&memory, scope).await,
                (1, Some(format!("ses-{scope}"))),
                "{scope}"
            );
        }
    }

    // issue-0017 SCN-001：刪除沒在執行的 Turn 排程，只清掉它自己的 scope。
    #[tokio::test]
    async fn deleting_an_idle_turn_job_clears_only_its_scope() {
        let memory = open_memory().await;
        let store = open_store().await;
        let job = claimed_turn(&store, 0).await;
        let own = scheduled_turn_scope(&job.id);
        let sibling = format!("{own}-x");
        for scope in [own.as_str(), sibling.as_str(), "user:tg-42"] {
            seed_scope(&memory, scope).await;
        }

        let outcome = delete_job(&store, &memory, &job.id).await.unwrap();

        assert_eq!(outcome, DeleteJobOutcome::Deleted);
        assert!(store.get_job(&job.id).await.unwrap().is_none());
        assert_eq!(scope_left(&memory, &own).await, (0, None));
        assert_eq!(
            scope_left(&memory, &sibling).await,
            (1, Some(format!("ses-{sibling}")))
        );
        assert_eq!(
            scope_left(&memory, "user:tg-42").await,
            (1, Some("ses-user:tg-42".to_string()))
        );
    }

    // issue-0017 SCN-006：維護排程與不存在的 id，行為不變。
    #[tokio::test]
    async fn deleting_a_maintenance_job_or_a_missing_id() {
        let memory = open_memory().await;
        let store = open_store().await;
        seed_scope(&memory, "project:T").await;
        let job = store
            .add_job(crate::job::NewJob {
                name: "prune".to_string(),
                kind: JobKind::Maintenance {
                    scope: Some("project:T".to_string()),
                    task: MaintenanceTask::Prune,
                },
                cron: "0 3 * * *".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            delete_job(&store, &memory, &job.id).await.unwrap(),
            DeleteJobOutcome::Deleted
        );
        assert_eq!(
            scope_left(&memory, "project:T").await,
            (1, Some("ses-project:T".to_string()))
        );
        assert_eq!(
            delete_job(&store, &memory, &job.id).await.unwrap(),
            DeleteJobOutcome::NotFound
        );
    }

    // issue-0017 SCN-003：執行中被刪除，回合照常成功，結束後才清 scope（含本回合寫入的記憶）。
    #[tokio::test]
    async fn deleting_a_running_job_cleans_up_after_the_turn() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = SlowBackend::new(1000, vec![Ok("oracle"), Ok("提醒完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 300).await;
        let own = scheduled_turn_scope(&job.id);

        let run = run_claimed_job(&store, &ctx, &job, "worker-a", 300);
        let delete = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            delete_job(&store, &memory, &job.id).await.unwrap()
        };
        let (outcome, deleted) = tokio::join!(run, delete);

        assert_eq!(deleted, DeleteJobOutcome::DeletedWhileHeld);
        match outcome.unwrap() {
            ClaimedJobOutcome::Deleted(output) => {
                assert!(output.success, "{}", output.message);
                assert_eq!(output.message, "提醒完成");
            }
            other => panic!("預期 Deleted，實際是 {other:?}"),
        }
        assert!(store.get_job(&job.id).await.unwrap().is_none());
        assert_eq!(scope_left(&memory, &own).await, (0, None));
    }

    // issue-0017 SCN-005：已被取得、還在排隊時被刪除，輪到時不執行，並清掉 scope。
    #[tokio::test]
    async fn deleting_a_queued_job_skips_it_and_clears_its_scope() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("不該執行")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 300).await;
        let own = scheduled_turn_scope(&job.id);
        seed_scope(&memory, &own).await;

        assert_eq!(
            delete_job(&store, &memory, &job.id).await.unwrap(),
            DeleteJobOutcome::DeletedWhileHeld
        );
        // 被持有時不清，留給 executor。
        assert_eq!(
            scope_left(&memory, &own).await,
            (1, Some(format!("ses-{own}")))
        );

        let outcome = run_claimed_job(&store, &ctx, &job, "worker-a", 300)
            .await
            .unwrap();

        assert_eq!(outcome, ClaimedJobOutcome::NotHeld);
        assert!(backend.prompts.lock().unwrap().is_empty());
        assert_eq!(scope_left(&memory, &own).await, (0, None));
    }

    // SCN-008：排隊期間 lease 被別人拿走的 job，輪到時不執行、不留 run 紀錄。
    #[tokio::test]
    async fn queued_job_whose_lease_was_taken_is_skipped() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("不該執行")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        // lease 0 秒：worker-a 取得後立刻過期，worker-b 接著取得。
        let job = claimed_turn(&store, 0).await;
        let own = scheduled_turn_scope(&job.id);
        seed_scope(&memory, &own).await;
        let now = chrono::Utc::now().timestamp();
        assert!(store
            .claim_job(&job.id, now, "worker-b", 300)
            .await
            .unwrap()
            .is_some());

        let outcome = run_claimed_job(&store, &ctx, &job, "worker-a", 300)
            .await
            .unwrap();

        assert_eq!(outcome, ClaimedJobOutcome::NotHeld);
        assert!(backend.prompts.lock().unwrap().is_empty());
        assert!(store
            .recent_runs(Some(&job.id), 10)
            .await
            .unwrap()
            .is_empty());
        // issue-0017：排程還在（只是被別人取得），不能清它的 scope。
        assert_eq!(
            scope_left(&memory, &own).await,
            (1, Some(format!("ses-{own}")))
        );
    }

    // SCN-001 的上限：daemon 預設 lease 300 秒，進度仍至少每 30 秒更新。
    #[test]
    fn heartbeat_is_capped_at_thirty_seconds() {
        assert_eq!(heartbeat_every(300), std::time::Duration::from_secs(30));
        assert_eq!(heartbeat_every(3), std::time::Duration::from_secs(1));
    }

    // SCN-001：執行中的 run 訊息顯示目前階段與已執行秒數，同一階段內也會更新。
    #[tokio::test]
    async fn running_run_shows_phase_and_elapsed_seconds() {
        let memory = open_memory().await;
        let store = open_store().await;
        // planner 與唯一一棒各 1.6 秒；lease 3 秒時每 1 秒更新一次進度。
        let backend = SlowBackend::new(1600, vec![Ok("oracle"), Ok("完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 3).await;

        let probe = async {
            let start = tokio::time::Instant::now();
            let mut messages = Vec::new();
            for at in [300u64, 1300, 2500] {
                tokio::time::sleep_until(start + std::time::Duration::from_millis(at)).await;
                let run = only_run(&store, &job).await;
                assert_eq!(run.status, RunStatus::Running);
                messages.push(run.message);
            }
            messages
        };
        let (outcome, messages) =
            tokio::join!(run_claimed_job(&store, &ctx, &job, "worker-a", 3), probe);
        outcome.unwrap();

        assert!(
            messages[0].starts_with("進行中：規劃中，已執行 "),
            "{messages:?}"
        );
        assert!(
            messages[1].starts_with("進行中：規劃中，已執行 "),
            "{messages:?}"
        );
        assert!(
            elapsed_secs(&messages[1]) >= 1,
            "同一階段內沒有更新：{messages:?}"
        );
        assert!(
            messages[2].starts_with("進行中：第 1 棒 oracle，已執行 "),
            "{messages:?}"
        );
    }

    // SCN-002：失敗時訊息開頭標出失敗的階段與已執行秒數，後面保留原錯誤。
    #[tokio::test]
    async fn failure_message_names_the_planning_phase() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = SlowBackend::new(0, vec![Err(timed_out())]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 300).await;

        run_claimed_job(&store, &ctx, &job, "worker-a", 300)
            .await
            .unwrap();

        let run = only_run(&store, &job).await;
        assert_eq!(run.status, RunStatus::Failure);
        assert!(
            run.message.starts_with("規劃中失敗，已執行 "),
            "{}",
            run.message
        );
        assert!(
            run.message.contains("本機 OpenCode 回合逾時，已中止程序。"),
            "{}",
            run.message
        );
    }

    #[tokio::test]
    async fn failure_message_names_the_failing_step() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = SlowBackend::new(
            0,
            vec![Ok("explorer, fixer"), Ok("看過了"), Err(timed_out())],
        );
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 300).await;

        run_claimed_job(&store, &ctx, &job, "worker-a", 300)
            .await
            .unwrap();

        let run = only_run(&store, &job).await;
        assert_eq!(run.status, RunStatus::Failure);
        assert!(
            run.message.starts_with("第 2 棒 fixer 失敗，已執行 "),
            "{}",
            run.message
        );
        assert!(
            run.message.contains("本機 OpenCode 回合逾時，已中止程序。"),
            "{}",
            run.message
        );
    }

    // SCN-006：在 lease 時間內結束的回合，run 訊息仍是最終輸出，lease 也照常釋放。
    #[tokio::test]
    async fn short_turn_result_and_leases_are_unchanged() {
        let memory = open_memory().await;
        let store = open_store().await;
        let backend = SlowBackend::new(0, vec![Ok("oracle"), Ok("完成")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let job = claimed_turn(&store, 300).await;

        run_claimed_job(&store, &ctx, &job, "worker-a", 300)
            .await
            .unwrap();

        let run = only_run(&store, &job).await;
        assert_eq!(run.status, RunStatus::Success);
        assert_eq!(run.message, "完成");
        let now = chrono::Utc::now().timestamp();
        assert!(store
            .claim_job(&job.id, now, "worker-b", 300)
            .await
            .unwrap()
            .is_some());
        assert!(memory
            .acquire_agent_session_lease(&scheduled_turn_scope(&job.id), "other", 300)
            .await
            .unwrap()
            .is_some());
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

    fn owned_turn_job(id: &str, prompt: &str) -> Job {
        Job {
            id: id.to_string(),
            ..job(JobKind::Turn {
                scope: "user:tg-42".to_string(),
                prompt: prompt.to_string(),
            })
        }
    }

    async fn seed_event(memory: &Memory, scope: &str, text: &str) {
        memory
            .remember(RememberInput {
                scope: scope.to_string(),
                session_id: None,
                items: vec![MemoryItem {
                    kind: MemoryKind::Event,
                    text: text.to_string(),
                    importance: None,
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
    }

    async fn event_texts(memory: &Memory, scope: &str) -> Vec<String> {
        memory
            .records(Some(scope), Some(MemoryKind::Event), 100)
            .await
            .unwrap()
            .records
            .into_iter()
            .map(|r| r.text)
            .collect()
    }

    /// 跑一次 Turn job（planner 一棒 + 執行一棒），回傳 backend 收到的所有 prompt。
    async fn run_turn_job(memory: &Memory, job: &Job, reply: &str) -> Vec<String> {
        let backend = MockBackend::new(vec![Ok("oracle"), Ok(reply)]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: PermissionPolicy::Reject,
        };
        let out = execute_job(&ctx, job).await;
        assert!(out.success, "{}", out.message);
        let prompts = backend.prompts.lock().unwrap().clone();
        prompts
    }

    // issue-0014 SCN-001
    #[tokio::test]
    async fn turn_job_remembers_in_its_own_scope_not_the_owner_scope() {
        let memory = open_memory().await;
        seed_event(&memory, "user:tg-42", "User: 早安").await;

        run_turn_job(
            &memory,
            &owned_turn_job("job-1", "summarize atlas progress"),
            "done",
        )
        .await;

        assert_eq!(event_texts(&memory, "user:tg-42").await, vec!["User: 早安"]);
        let own = event_texts(&memory, "agent:schedule-job-1").await;
        assert_eq!(own.len(), 2, "{own:?}");
        assert!(own
            .iter()
            .any(|t| t.starts_with("User: summarize atlas progress")));
        assert!(own.contains(&"Assistant: done".to_string()));
    }

    // issue-0014 SCN-002
    #[tokio::test]
    async fn turn_job_neither_recalls_nor_resumes_the_owner_scope() {
        let memory = open_memory().await;
        seed_event(
            &memory,
            "user:tg-42",
            "Assistant: atlas progress reports use tables",
        )
        .await;
        memory
            .set_agent_session("user:tg-42", "ses_user")
            .await
            .unwrap();

        let prompts = run_turn_job(
            &memory,
            &owned_turn_job("job-1", "summarize atlas progress"),
            "done",
        )
        .await;

        assert!(
            !prompts.iter().any(|p| p.contains("use tables")),
            "{prompts:?}"
        );
        assert_eq!(
            memory.agent_session("user:tg-42").await.unwrap().as_deref(),
            Some("ses_user")
        );
        assert_eq!(
            memory
                .agent_session("agent:schedule-job-1")
                .await
                .unwrap()
                .as_deref(),
            Some("ses_new")
        );
        assert_eq!(memory.agent_session("project:Base").await.unwrap(), None);
    }

    // issue-0014 SCN-004
    #[tokio::test]
    async fn each_turn_job_continues_only_its_own_scope() {
        let memory = open_memory().await;
        let alpha = owned_turn_job("job-1", "alpha report");
        let beta = owned_turn_job("job-2", "beta report");

        run_turn_job(&memory, &alpha, "alpha-first").await;
        run_turn_job(&memory, &beta, "beta-first").await;
        let alpha_second = run_turn_job(&memory, &alpha, "alpha-second")
            .await
            .join("\n");
        let beta_second = run_turn_job(&memory, &beta, "beta-second").await.join("\n");

        assert!(alpha_second.contains("Assistant: alpha-first"));
        assert!(!alpha_second.contains("beta-first"));
        assert!(beta_second.contains("Assistant: beta-first"));
        assert!(!beta_second.contains("alpha-first"));
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

    /// AC-6：上游錯誤導致的排程失敗，紀錄要說得出原因——這正是 TeleNexus 那份
    /// 100% success rate 的來源：模型下架在排程紀錄裡長得跟成功一模一樣。
    #[tokio::test]
    async fn turn_job_reports_upstream_failure_with_its_classification() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Err(GatewayError::UpstreamFailed {
            kind: wukong_gateway::UpstreamFailure::ModelEol,
            status_code: Some(410),
            detail: "APIError statusCode=410: model reached end of life".to_string(),
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

        assert!(!out.success, "上游下架不得記成成功");
        assert!(
            out.message
                .contains(wukong_gateway::UpstreamFailure::ModelEol.label()),
            "{}",
            out.message
        );
        assert!(out.message.contains("410"), "{}", out.message);
    }

    /// SCN-001：排程遇到模型免費額度不足時，排程結果為 failure 且包含額度不足的原始原因。
    #[tokio::test]
    async fn turn_job_reports_quota_exhaustion_failure_with_cause() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Err(GatewayError::UpstreamFailed {
            kind: wukong_gateway::UpstreamFailure::RateLimited,
            status_code: Some(429),
            detail: "AI_APICallError: Free usage limit reached. Free models work in OpenCode"
                .to_string(),
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

        assert!(!out.success, "額度耗盡排程結果必須為 failure");
        assert!(
            out.message
                .contains(wukong_gateway::UpstreamFailure::RateLimited.label()),
            "message 應包含分類標籤: {}",
            out.message
        );
        assert!(
            out.message.contains("Free usage limit reached"),
            "message 應包含額度原因: {}",
            out.message
        );
    }

    /// AC-6 失敗路徑：一般 backend 失敗不得被貼上上游錯誤的分類。
    #[tokio::test]
    async fn ordinary_backend_failure_is_not_labelled_as_upstream() {
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
        assert!(!out
            .message
            .contains(wukong_gateway::UpstreamFailure::ModelEol.label()));
        assert!(!out
            .message
            .contains(wukong_gateway::UpstreamFailure::RateLimited.label()));
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
