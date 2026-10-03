use wukong_gateway::backend::{AgentBackend, AgentCliBackend};
use wukong_gateway::config::GatewayConfig;

pub fn fixture(scope: &str) -> (tempfile::TempDir, AgentBackend, GatewayConfig) {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("agent.sh");
    std::fs::write(
        &script,
        r#"state=$1
shift
session=''
stream=false
empty=false
for arg in "$@"; do
    printf '%s\n' "$arg" >> "$state/argv"
    case "$arg" in *EMPTY_OUTPUT_PROBE*) empty=true ;; esac
done
while [ "$#" -gt 0 ]; do
    case "$1" in
        -s) session=$2; shift ;;
        --format) stream=true; shift ;;
    esac
    shift
done
if [ "$stream" = false ]; then
    printf 'fixer|none\n'
    exit 0
fi
printf 'resumed=%s\n' "$session" >> "$state/sessions"
session=${session:-ses_$$}
printf '{"type":"step_start","sessionID":"%s"}\n' "$session"
printf '{"type":"tool_use","part":{"tool":"read"}}\n'
sleep 0.1
if [ "$empty" = false ]; then
    printf '{"type":"text","part":{"text":"CLI_INTEGRATION_OK"}}\n'
fi
sleep 0.5
touch "$state/finished"
printf '{"type":"step_finish"}\n'
"#,
    )
    .unwrap();
    let script = std::env::var_os("WUKONG_ISSUE7_OPENCODE_PROBE")
        .map(std::path::PathBuf::from)
        .unwrap_or(script);
    let command = vec![
        "sh".to_string(),
        script.to_string_lossy().into_owned(),
        dir.path().to_string_lossy().into_owned(),
    ];
    let cfg = GatewayConfig {
        scope: scope.to_string(),
        db_url: format!("sqlite://{}", dir.path().join("memory.db").display()),
        agent_command: command.clone(),
        default_model: None,
        planner_preferences: None,
        thinking: false,
        recall_top_k: 5,
        stream: true,
    };
    let backend = AgentBackend::Cli(AgentCliBackend {
        command,
        workspace: Some(dir.path().to_path_buf()),
    });
    (dir, backend, cfg)
}
