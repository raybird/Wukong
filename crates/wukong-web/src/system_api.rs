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
                    Some(
                        "Set a memory database URL before relying on memory diagnostics"
                            .to_string(),
                    ),
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
            DiagnosticItem::ok(
                "total",
                "Total schedules",
                &schedule_total.to_string(),
                None,
            ),
            DiagnosticItem::ok(
                "enabled",
                "Enabled schedules",
                &schedule_enabled.to_string(),
                None,
            ),
            DiagnosticItem::ok(
                "next_run_at",
                "Next run",
                &next_run_at
                    .map(|ts| ts.to_string())
                    .unwrap_or_else(|| "not scheduled".to_string()),
                None,
            ),
        ],
    }
}

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
        assert!(response
            .groups
            .iter()
            .any(|group| group.id == "environment"));
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

    #[test]
    fn command_success_becomes_ok_item_with_summary() {
        let item = command_diagnostic_item(
            "providers",
            "Providers",
            Ok("opencode\nanthropic".to_string()),
        );

        assert_eq!(item.status, DiagnosticStatus::Ok);
        assert_eq!(item.summary, "opencode");
        assert_eq!(item.detail.as_deref(), Some("opencode\nanthropic"));
    }

    #[test]
    fn command_failure_becomes_warn_item() {
        let item =
            command_diagnostic_item("models", "Models", Err("backend unavailable".to_string()));

        assert_eq!(item.status, DiagnosticStatus::Warn);
        assert_eq!(item.summary, "backend unavailable");
        assert!(item.suggestion.as_deref().unwrap().contains("Retry"));
    }
}
