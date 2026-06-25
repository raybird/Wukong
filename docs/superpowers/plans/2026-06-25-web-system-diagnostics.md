# Web System Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the Web Console System tab into a read-only runtime diagnostics dashboard with grouped status cards.

**Architecture:** Keep `GET /api/system` as the single endpoint and preserve existing top-level fields. Add diagnostic response types and builders in `crates/wukong-web/src/system_api.rs`, wire async command checks from `crates/wukong-web/src/lib.rs`, and render groups in `crates/wukong-web/static/components/wukong-system.js`.

**Tech Stack:** Rust, axum, tokio, serde, existing `wukong_gateway::backend::OpencodeUtility`, zero-build JavaScript Web Components.

---

## File Map

- Modify `crates/wukong-web/src/system_api.rs`: owns serializable response types, pure diagnostic item builders, environment/schedule group builders, and tests for response shape.
- Modify `crates/wukong-web/src/lib.rs`: keeps token/scheduler route flow, runs async providers/models/GitHub diagnostics with timeout, and passes groups into the response.
- Modify `crates/wukong-web/static/components/wukong-system.js`: renders summary cards, diagnostic group cards, status badges, details, suggestions, and refresh button.
- No new endpoint files. Do not split `/api/system` in this first version.
- Do not modify settings persistence or provider credentials.

## Task 1: Add Diagnostic Response Types And Pure Groups

**Files:**
- Modify: `crates/wukong-web/src/system_api.rs`

- [ ] **Step 1: Run impact analysis before editing `SystemResponse`**

Run GitNexus impact:

```text
gitnexus_impact({ target: "SystemResponse", direction: "upstream", file_path: "crates/wukong-web/src/system_api.rs", kind: "Struct", repo: "Wukong" })
gitnexus_impact({ target: "system_response", direction: "upstream", file_path: "crates/wukong-web/src/system_api.rs", kind: "Function", repo: "Wukong" })
```

Expected: report affected route/test surface before editing. If risk is HIGH or CRITICAL, tell the user before continuing.

- [ ] **Step 2: Write failing tests for grouped diagnostics**

Add these tests at the bottom of `crates/wukong-web/src/system_api.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wukong_scheduler::{Job, JobKind, MaintenanceTask};

    fn job(enabled: bool, next_run_at: Option<i64>) -> Job {
        Job {
            id: "job-1".to_string(),
            name: "snapshot".to_string(),
            kind: JobKind::Maintenance {
                scope: Some("global".to_string()),
                task: MaintenanceTask::Snapshot,
            },
            cron: "0 3 * * *".to_string(),
            enabled,
            next_run_at,
            last_run_at: None,
        }
    }

    #[test]
    fn system_response_preserves_summary_and_adds_groups() {
        let response = system_response(
            "global",
            true,
            "sqlite://memory.db",
            &[job(true, Some(200))],
            vec![DiagnosticGroup {
                id: "providers".to_string(),
                title: "Providers".to_string(),
                items: vec![DiagnosticItem::ok(
                    "providers",
                    "Providers",
                    "available",
                    Some("opencode".to_string()),
                )],
            }],
        );

        assert_eq!(response.scope, "global");
        assert!(response.token_enabled);
        assert_eq!(response.memory_db, "configured");
        assert_eq!(response.schedule_total, 1);
        assert_eq!(response.schedule_enabled, 1);
        assert_eq!(response.next_run_at, Some(200));
        assert!(response.groups.iter().any(|group| group.id == "runtime"));
        assert!(response.groups.iter().any(|group| group.id == "environment"));
        assert!(response.groups.iter().any(|group| group.id == "schedules"));
        assert!(response.groups.iter().any(|group| group.id == "providers"));
    }

    #[test]
    fn diagnostic_status_serializes_as_lowercase() {
        let item = DiagnosticItem::warn(
            "github",
            "GitHub CLI",
            "not authenticated",
            None,
            Some("Run gh auth login".to_string()),
        );
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""status":"warn""#), "json: {json}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-web system_api::tests::system_response_preserves_summary_and_adds_groups -- --nocapture
cargo test -p wukong-web system_api::tests::diagnostic_status_serializes_as_lowercase -- --nocapture
```

Expected: FAIL because `DiagnosticGroup`, `DiagnosticItem`, `DiagnosticStatus`, and the new `system_response` signature do not exist.

- [ ] **Step 4: Implement minimal diagnostic response types**

Replace `crates/wukong-web/src/system_api.rs` with this structure, preserving imports at the top as needed:

```rust
use serde::Serialize;
use wukong_scheduler::Job;

#[derive(Debug, Serialize)]
pub struct SystemResponse {
    pub scope: String,
    pub token_enabled: bool,
    pub memory_db: String,
    pub schedule_total: usize,
    pub schedule_enabled: usize,
    pub next_run_at: Option<i64>,
    pub groups: Vec<DiagnosticGroup>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticItem {
    pub id: String,
    pub label: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl DiagnosticItem {
    pub fn ok(id: &str, label: &str, summary: &str, detail: Option<String>) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Ok,
            summary: summary.to_string(),
            detail,
            suggestion: None,
        }
    }

    pub fn warn(
        id: &str,
        label: &str,
        summary: &str,
        detail: Option<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Warn,
            summary: summary.to_string(),
            detail,
            suggestion,
        }
    }

    pub fn error(
        id: &str,
        label: &str,
        summary: &str,
        detail: Option<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: DiagnosticStatus::Error,
            summary: summary.to_string(),
            detail,
            suggestion,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticGroup {
    pub id: String,
    pub title: String,
    pub items: Vec<DiagnosticItem>,
}

pub fn system_response(
    scope: &str,
    token_enabled: bool,
    db_url: &str,
    jobs: &[Job],
    extra_groups: Vec<DiagnosticGroup>,
) -> SystemResponse {
    let memory_db = if db_url.trim().is_empty() {
        "unavailable".to_string()
    } else {
        "configured".to_string()
    };
    let schedule_total = jobs.len();
    let schedule_enabled = jobs.iter().filter(|j| j.enabled).count();
    let next_run_at = jobs.iter().filter_map(|j| j.next_run_at).min();

    let mut groups = vec![
        runtime_group(scope, token_enabled),
        environment_group(db_url),
        schedules_group(schedule_total, schedule_enabled, next_run_at),
    ];
    groups.extend(extra_groups);

    SystemResponse {
        scope: scope.to_string(),
        token_enabled,
        memory_db,
        schedule_total,
        schedule_enabled,
        next_run_at,
        groups,
    }
}

fn runtime_group(scope: &str, token_enabled: bool) -> DiagnosticGroup {
    DiagnosticGroup {
        id: "runtime".to_string(),
        title: "Runtime".to_string(),
        items: vec![
            DiagnosticItem::ok("scope", "Scope", scope, None),
            DiagnosticItem::ok(
                "web_token",
                "Web token",
                if token_enabled { "enabled" } else { "disabled" },
                None,
            ),
        ],
    }
}

fn environment_group(db_url: &str) -> DiagnosticGroup {
    let workspace = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|e| format!("unavailable: {e}"));
    let container_hint = if std::path::Path::new("/.dockerenv").exists() {
        "container detected"
    } else {
        "host process"
    };
    DiagnosticGroup {
        id: "environment".to_string(),
        title: "Environment".to_string(),
        items: vec![
            DiagnosticItem::ok("workspace", "Workspace", &workspace, None),
            if db_url.trim().is_empty() {
                DiagnosticItem::warn(
                    "memory_db",
                    "Memory DB",
                    "unavailable",
                    None,
                    Some("Set a memory database URL before relying on memory diagnostics".to_string()),
                )
            } else {
                DiagnosticItem::ok("memory_db", "Memory DB", "configured", None)
            },
            DiagnosticItem::ok("container", "Container", container_hint, None),
        ],
    }
}

fn schedules_group(
    schedule_total: usize,
    schedule_enabled: usize,
    next_run_at: Option<i64>,
) -> DiagnosticGroup {
    DiagnosticGroup {
        id: "schedules".to_string(),
        title: "Schedules".to_string(),
        items: vec![
            DiagnosticItem::ok("total", "Total schedules", &schedule_total.to_string(), None),
            DiagnosticItem::ok("enabled", "Enabled schedules", &schedule_enabled.to_string(), None),
            DiagnosticItem::ok(
                "next_run_at",
                "Next run",
                &next_run_at.map(|ts| ts.to_string()).unwrap_or_else(|| "not scheduled".to_string()),
                None,
            ),
        ],
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test -p wukong-web system_api::tests::system_response_preserves_summary_and_adds_groups -- --nocapture
cargo test -p wukong-web system_api::tests::diagnostic_status_serializes_as_lowercase -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Update route call site for new signature**

In `crates/wukong-web/src/lib.rs`, update the existing `system_api::system_response` call in `get_system` to pass an empty extra group list:

```rust
Ok(jobs) => Json(system_api::system_response(
    &state.scope,
    state.token.is_some(),
    &state.db_url,
    &jobs,
    Vec::new(),
))
.into_response(),
```

- [ ] **Step 7: Run existing System route test**

Run:

```bash
cargo test -p wukong-web system_returns_summary -- --nocapture
```

Expected: PASS and existing assertions for `scope`, `token_enabled`, and `schedule_total` remain valid.

- [ ] **Step 8: Commit Task 1**

Run:

```bash
git add crates/wukong-web/src/system_api.rs crates/wukong-web/src/lib.rs
git commit -m "feat(web): add system diagnostic response shape"
```

Expected: commit succeeds. Do not stage unrelated files.

## Task 2: Add Providers, Models, And GitHub Diagnostic Builders

**Files:**
- Modify: `crates/wukong-web/src/system_api.rs`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing route helper behavior**

Run GitNexus impact:

```text
gitnexus_impact({ target: "get_system", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", kind: "Function", repo: "Wukong" })
```

Expected: report affected route/test surface. Warn the user before continuing if risk is HIGH or CRITICAL.

- [ ] **Step 2: Write failing pure tests for command diagnostic items**

Add these tests inside `system_api.rs` test module:

```rust
#[test]
fn command_success_becomes_ok_item_with_summary() {
    let item = command_diagnostic_item("providers", "Providers", Ok("opencode\nanthropic".to_string()));

    assert_eq!(item.status, DiagnosticStatus::Ok);
    assert_eq!(item.summary, "opencode");
    assert_eq!(item.detail.as_deref(), Some("opencode\nanthropic"));
}

#[test]
fn command_failure_becomes_warn_item() {
    let item = command_diagnostic_item(
        "models",
        "Models",
        Err("backend unavailable".to_string()),
    );

    assert_eq!(item.status, DiagnosticStatus::Warn);
    assert_eq!(item.summary, "backend unavailable");
    assert!(item.suggestion.as_deref().unwrap().contains("Retry"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-web system_api::tests::command_success_becomes_ok_item_with_summary -- --nocapture
cargo test -p wukong-web system_api::tests::command_failure_becomes_warn_item -- --nocapture
```

Expected: FAIL because `command_diagnostic_item` does not exist.

- [ ] **Step 4: Implement command item helper and output summarizer**

Add these functions to `crates/wukong-web/src/system_api.rs`:

```rust
pub fn command_diagnostic_item(
    id: &str,
    label: &str,
    result: Result<String, String>,
) -> DiagnosticItem {
    match result {
        Ok(output) => {
            let summary = summarize_output(&output);
            DiagnosticItem::ok(id, label, &summary, Some(output))
        }
        Err(error) => DiagnosticItem::warn(
            id,
            label,
            &error,
            None,
            Some("Retry from System or check the backend command in the terminal".to_string()),
        ),
    }
}

fn summarize_output(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .chars()
        .take(120)
        .collect()
}
```

- [ ] **Step 5: Run pure command tests to verify they pass**

Run:

```bash
cargo test -p wukong-web system_api::tests::command_success_becomes_ok_item_with_summary -- --nocapture
cargo test -p wukong-web system_api::tests::command_failure_becomes_warn_item -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Write a failing route test for diagnostic groups shape**

Add this test near `system_returns_summary` in `crates/wukong-web/src/lib.rs`:

```rust
#[tokio::test]
async fn system_returns_diagnostic_groups() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(Request::builder().uri("/api/system").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""groups""#), "body: {body}");
    assert!(body.contains(r#""id":"runtime""#), "body: {body}");
    assert!(body.contains(r#""id":"environment""#), "body: {body}");
    assert!(body.contains(r#""id":"schedules""#), "body: {body}");
}
```

- [ ] **Step 7: Run route regression test**

Run:

```bash
cargo test -p wukong-web system_returns_diagnostic_groups -- --nocapture
```

Expected: PASS. Task 1 already added the base diagnostic groups to `/api/system`; this test keeps that shape covered while Task 2 adds command-backed groups.

- [ ] **Step 8: Add production command diagnostic runners**

In `crates/wukong-web/src/lib.rs`, add helper functions above `get_system`:

```rust
const SYSTEM_DIAGNOSTIC_TIMEOUT_SECS: u64 = 5;

async fn run_opencode_diagnostic(args: &[&str]) -> Result<String, String> {
    let util = wukong_gateway::backend::OpencodeUtility::from_agent_command(
        &[],
        wukong_gateway::workspace_dir(),
    );
    match tokio::time::timeout(
        std::time::Duration::from_secs(SYSTEM_DIAGNOSTIC_TIMEOUT_SECS),
        util.run_fixed(args),
    )
    .await
    {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

async fn run_github_auth_status() -> Result<String, String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(SYSTEM_DIAGNOSTIC_TIMEOUT_SECS),
        tokio::process::Command::new("gh").args(["auth", "status"]).output(),
    )
    .await
    {
        Ok(Ok(output)) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if stderr.is_empty() { "gh auth status failed".to_string() } else { stderr })
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}
```

Then add production group builder above `get_system`. Run providers, models, and GitHub checks concurrently so the route waits at most one timeout window in the common slow-command case:

```rust
async fn system_extra_groups() -> Vec<system_api::DiagnosticGroup> {
    let (providers_result, models_result, github_result) = tokio::join!(
        run_opencode_diagnostic(&["providers", "list"]),
        run_opencode_diagnostic(&["models"]),
        run_github_auth_status(),
    );

    let providers = system_api::command_diagnostic_item(
        "providers",
        "Providers",
        providers_result,
    );
    let models = system_api::command_diagnostic_item(
        "models",
        "Models",
        models_result,
    );
    let github = system_api::command_diagnostic_item(
        "github_cli",
        "GitHub CLI",
        github_result,
    );

    vec![
        system_api::DiagnosticGroup {
            id: "providers".to_string(),
            title: "Providers".to_string(),
            items: vec![providers],
        },
        system_api::DiagnosticGroup {
            id: "models".to_string(),
            title: "Models".to_string(),
            items: vec![models],
        },
        system_api::DiagnosticGroup {
            id: "tools".to_string(),
            title: "Tools".to_string(),
            items: vec![github],
        },
    ]
}
```

- [ ] **Step 9: Update `get_system` to pass extra groups**

In `get_system`, before `Json(system_api::system_response(...))`, collect groups:

```rust
Ok(jobs) => {
    let groups = system_extra_groups().await;
    Json(system_api::system_response(
        &state.scope,
        state.token.is_some(),
        &state.db_url,
        &jobs,
        groups,
    ))
    .into_response()
}
```

- [ ] **Step 10: Run System tests**

Run:

```bash
cargo test -p wukong-web system_api::tests -- --nocapture
cargo test -p wukong-web system_returns_summary -- --nocapture
cargo test -p wukong-web system_returns_diagnostic_groups -- --nocapture
```

Expected: PASS. The route test does not assert real providers/models command output, so it remains stable on machines without configured providers.

- [ ] **Step 11: Commit Task 2**

Run:

```bash
git add crates/wukong-web/src/system_api.rs crates/wukong-web/src/lib.rs
git commit -m "feat(web): add system runtime diagnostics"
```

Expected: commit succeeds. Do not stage unrelated files.

## Task 3: Render System Dashboard Cards

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-system.js`

- [ ] **Step 1: Run impact analysis before editing `WukongSystem` methods**

Run GitNexus impact:

```text
gitnexus_impact({ target: "WukongSystem", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-system.js", kind: "Class", repo: "Wukong" })
```

Expected: report affected frontend component flow before editing.

- [ ] **Step 2: Replace `wukong-system.js` rendering with dashboard layout**

Replace `crates/wukong-web/static/components/wukong-system.js` with:

```javascript
import { html, unsafe } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>系統</h2>
            <p class="panel-help">Read-only runtime diagnostics for backend reachability, tools, environment, and schedules.</p>
          </div>
          <button id="refresh-system" type="button">重新整理</button>
        </div>
        <div id="system-status" class="settings-status">載入中…</div>
        <div id="system-summary" class="stat-grid"></div>
        <div id="system-groups" class="record-list"></div>
      </section>
    `.toString();
    this.status = this.querySelector('#system-status');
    this.summary = this.querySelector('#system-summary');
    this.groups = this.querySelector('#system-groups');
    this.querySelector('#refresh-system').addEventListener('click', () => this.load());
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    this.status.textContent = '載入中…';
    const resp = await fetch('/api/system' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取系統資訊：HTTP ' + resp.status;
      this.summary.innerHTML = '';
      this.groups.innerHTML = '';
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入系統診斷';
    this.renderSummary(data);
    this.renderGroups(data.groups || []);
  }

  renderSummary(data) {
    const next = data.next_run_at ? new Date(data.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    this.summary.innerHTML = html`
      <article class="stat-card"><span>Scope</span><strong>${data.scope}</strong></article>
      <article class="stat-card"><span>Web token</span><strong>${data.token_enabled ? '已啟用' : '未啟用'}</strong></article>
      <article class="stat-card"><span>Memory DB</span><strong>${data.memory_db}</strong></article>
      <article class="stat-card"><span>排程</span><strong>${data.schedule_enabled}/${data.schedule_total}</strong></article>
      <article class="stat-card"><span>最近下次執行</span><strong>${next}</strong></article>
    `.toString();
  }

  renderGroups(groups) {
    this.groups.innerHTML = groups.map((group) => html`
      <section class="control-card">
        <h3>${group.title}</h3>
        <div class="record-list">
          ${(group.items || []).map((item) => unsafe(this.renderItem(item)))}
        </div>
      </section>
    `.toString()).join('') || '<p class="empty-state">沒有診斷資料。</p>';
  }

  renderItem(item) {
    return html`
      <article class="record-card system-diagnostic ${this.statusClass(item.status)}">
        <div><span class="tag">${item.status || 'unknown'}</span> <strong>${item.label}</strong></div>
        <p>${item.summary || ''}</p>
        ${item.detail ? html`<small>${item.detail}</small>` : ''}
        ${item.suggestion ? html`<small>建議：${item.suggestion}</small>` : ''}
      </article>
    `.toString();
  }

  statusClass(status) {
    if (status === 'ok') return 'system-ok';
    if (status === 'warn') return 'system-warn';
    if (status === 'error') return 'system-error';
    return 'system-unknown';
  }
}
```

- [ ] **Step 3: Run JavaScript syntax check**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-system.js
```

Expected: no output and exit 0.

- [ ] **Step 4: Run web tests**

Run:

```bash
cargo test -p wukong-web system_returns_summary -- --nocapture
cargo test -p wukong-web serves_static_assets_with_content_types -- --nocapture
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

Run:

```bash
git add crates/wukong-web/static/components/wukong-system.js
git commit -m "feat(web): render system diagnostics dashboard"
```

Expected: commit succeeds. Do not stage unrelated files.

## Task 4: Final Verification And Change Review

**Files:**
- Review all files changed by Tasks 1-3.

- [ ] **Step 1: Format Rust code**

Run:

```bash
cargo fmt
```

Expected: no output and exit 0.

- [ ] **Step 2: Run targeted checks**

Run:

```bash
cargo test -p wukong-web system_api::tests -- --nocapture
cargo test -p wukong-web system_returns_summary -- --nocapture
node --check crates/wukong-web/static/components/wukong-system.js
```

Expected: PASS / no JS syntax output.

- [ ] **Step 3: Run full verification**

Run:

```bash
cargo test -p wukong-web
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass and clippy finishes with no warnings.

- [ ] **Step 4: Run GitNexus change detection before final completion**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols are limited to Web system diagnostics files plus any explicitly approved incidental changes. If `AGENTS.md` or `CLAUDE.md` are still dirty and unrelated, do not stage them unless the user explicitly asks.

- [ ] **Step 5: Inspect git status and diff**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended files are dirty, or the workspace is clean if all task commits were made.

- [ ] **Step 6: Report completion evidence**

Report:

- commits created,
- exact verification commands run,
- GitNexus risk summary,
- any remaining dirty files that were not part of the task.
