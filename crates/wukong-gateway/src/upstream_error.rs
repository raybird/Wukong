//! 上游（provider）錯誤的分類——gateway 內**唯一**的判定真相來源。
//!
//! 背景見 `docs/superpowers/specs/2026-09-08-model-eol-silent-failure-remediation-design.md`。
//! 一句話：`exit code == 0` 與 `HTTP 200` 都不代表上游服務了這次請求。模型被下架時
//! opencode 會吞掉錯誤、吐一段降級文字，然後正常收場。
//!
//! 判定順序刻意固定為**先結構化訊號、後文字**：
//!
//! 1. [`classify`] 吃 opencode 事件裡具名錯誤的 `name` 與 `data.statusCode`。
//!    `statusCode` 是 integer 欄位（見 opencode `/doc` 的 `APIError` schema），
//!    所以這條路是型別安全的整數比較，結構上不可能被模型輸出裡的數字誤觸。
//! 2. [`classify_text`] 只在退無可退的純文字路徑（非串流 `run`）作為第二道。
//!
//! 這個順序來自 Wukong 既有的驗證紀律：診斷訊號不可與被診斷的機制同源，要用**結構性**
//! 訊號。文字比對是同源的（模型自己就能產出那些字），所以它只能當退路，不能當主判。

/// 一則來自上游的具名錯誤，欄位原樣取自 opencode 事件，不做任何文字解析。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamError {
    /// opencode 的錯誤名，如 `APIError`、`MessageAbortedError`。
    pub name: String,
    /// `data.statusCode`（僅 `APIError` 帶）。這是主要判定依據。
    pub status_code: Option<u16>,
    /// `data.message`，僅供人閱讀，不參與判定。
    pub message: String,
}

impl UpstreamError {
    /// 轉成 gateway 錯誤。分類只吃結構化欄位（`name` 與 `statusCode`），文字只進
    /// detail 供人閱讀，不參與判定。
    pub fn to_gateway_error(&self) -> crate::error::GatewayError {
        let kind = classify(&self.name, self.status_code);
        let mut detail = self.name.clone();
        if let Some(code) = self.status_code {
            detail.push_str(&format!(" statusCode={code}"));
        }
        if !self.message.is_empty() {
            detail.push_str(&format!(": {}", self.message));
        }
        crate::error::GatewayError::UpstreamFailed {
            kind,
            status_code: self.status_code,
            detail,
        }
    }
}

/// 從帶有 `error` 欄位的容器取出具名錯誤。兩條路徑共用同一份解析，形狀走鐘的風險
/// 才不會只在其中一條顯形：
///
/// - Server SSE：`session.error` 事件的 `properties`
/// - CLI `--format json`：`{"type":"error", ...}` 那一行的根物件
///
/// 兩者實測（2026-09-08，opencode 1.18.18）都是
/// `{"error":{"name":"…","data":{"message":"…","statusCode":410}}}`。
///
/// OpenCode V2 的 CLI 同樣送 `{"type":"error","error":{…}}`，但 `error` 換成
/// `{type, message, status?}`；沒有 `name` 而有 `type` 時改用 [`parse_v2_error`]。
pub fn parse_error_field(container: &serde_json::Value) -> Option<UpstreamError> {
    use serde_json::Value;
    let error = container.get("error")?;
    if error.get("name").is_none() && error.get("type").is_some() {
        return parse_v2_error(error);
    }
    let name = error
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("UnknownError")
        .to_string();
    let data = error.get("data");
    let status_code = data
        .and_then(|data| data.get("statusCode"))
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok());
    let message = data
        .and_then(|data| data.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(UpstreamError {
        name,
        status_code,
        message,
    })
}

/// OpenCode V2 的錯誤物件 `{type, message, status?, retryAfterMs?}`（openapi 的
/// `Session.StructuredError`）。`status` 是整數的 HTTP 狀態碼，與 V1 的 `statusCode` 同義。
pub fn parse_v2_error(error: &serde_json::Value) -> Option<UpstreamError> {
    use serde_json::Value;
    let name = error.get("type")?.as_str()?.to_string();
    Some(UpstreamError {
        name,
        status_code: error
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|code| u16::try_from(code).ok()),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 上游錯誤的分類。決定告警怎麼說、以及是否值得重試。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamFailure {
    /// 模型已下架（410）。換模型才會好，重試無用。
    ModelEol,
    /// 上游限流（429）。稍後重試可能會好。
    RateLimited,
    /// 回合被中止。不是上游的錯，但也**不是成功**。
    Aborted,
    /// 連不到上游（網路、DNS、逾時）。V2 會一直重試，由 [`RetryWatch`] 收斂。
    Unreachable,
    /// 其他上游錯誤。
    Other,
}

impl UpstreamFailure {
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelEol => "模型已下架",
            Self::RateLimited => "上游限流",
            Self::Aborted => "回合被中止",
            Self::Unreachable => "上游連不到",
            Self::Other => "上游錯誤",
        }
    }
}

impl std::fmt::Display for UpstreamFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// 主判定：先看結構化的 `statusCode`，取不到才退回錯誤名稱。
///
/// **刻意不認 404。** 非對話類模型（embedding、圖像、語音）打 chat/completions 也回
/// 404，那是「用錯端點」不是「模型失效」；混進來會讓告警說錯原因。
pub fn classify(name: &str, status_code: Option<u16>) -> UpstreamFailure {
    match status_code {
        Some(410) => return UpstreamFailure::ModelEol,
        Some(429) => return UpstreamFailure::RateLimited,
        _ => {}
    }
    match name {
        // V1 的錯誤名與 V2 的錯誤類型（`step.failed` 的 `error.type`、execution 中止）。
        "MessageAbortedError" | "aborted" => UpstreamFailure::Aborted,
        "ProviderModelNotFoundError" => UpstreamFailure::ModelEol,
        // V2 2.3.2 的 provider 錯誤類型（取自 2.3.2 執行檔的錯誤對應；上游回 410、429 時
        // `status` 照樣帶狀態碼，由上面先判）。額度、預算與各方案用量上限都歸為限流。
        "provider.rate-limit"
        | "provider.quota"
        | "provider.free-limit"
        | "provider.go-limit"
        | "provider.budget-limit"
        | "provider.model-budget-limit"
        | "provider.chatgpt-limit" => UpstreamFailure::RateLimited,
        // 找不到可用的模型路由（實測：不存在的模型），與 V1 的 ProviderModelNotFoundError 同類。
        "provider.no-route" => UpstreamFailure::ModelEol,
        "provider.transport" | "provider.connect" | "provider.timeout" => {
            UpstreamFailure::Unreachable
        }
        _ => UpstreamFailure::Other,
    }
}

/// V2 遇到上游錯誤時會一直重試（實測：連不到上游時 120 秒內重試 7 次仍未結束，V1 約 65
/// 秒就以錯誤結束），Wukong 在重試持續這麼久之後中止回合（issue-0044 TBD-3）。
pub const RETRY_GIVE_UP_AFTER: std::time::Duration = std::time::Duration::from_secs(90);

/// 追蹤一次回合中持續的重試。第一次重試起算，模型有了實際輸出（思考、文字、工具、
/// 步驟結束）就算恢復並重新起算；超過時限時以最後一次重試的錯誤回報。
#[derive(Debug)]
pub struct RetryWatch {
    limit: std::time::Duration,
    since: Option<tokio::time::Instant>,
    attempts: u32,
    last: Option<UpstreamError>,
}

impl Default for RetryWatch {
    fn default() -> Self {
        Self::new(RETRY_GIVE_UP_AFTER)
    }
}

impl RetryWatch {
    pub fn new(limit: std::time::Duration) -> Self {
        Self {
            limit,
            since: None,
            attempts: 0,
            last: None,
        }
    }

    pub fn retrying(&mut self, error: Option<UpstreamError>) {
        self.since.get_or_insert_with(tokio::time::Instant::now);
        self.attempts += 1;
        if error.is_some() {
            self.last = error;
        }
    }

    pub fn recovered(&mut self) {
        self.since = None;
        self.attempts = 0;
        self.last = None;
    }

    /// 重試中才有值：到這個時間仍未恢復就中止。
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.since.map(|since| since + self.limit)
    }

    /// 等到 [`Self::deadline`]；沒有在重試時永遠不會完成，可直接放進 `select!`。
    pub async fn expired(&self) {
        match self.deadline() {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    pub fn give_up(&self) -> crate::error::GatewayError {
        let error = self.last.clone().unwrap_or(UpstreamError {
            name: "UnknownError".to_string(),
            status_code: None,
            message: String::new(),
        });
        let mut gateway_error = error.to_gateway_error();
        if let crate::error::GatewayError::UpstreamFailed { detail, .. } = &mut gateway_error {
            detail.push_str(&format!(
                "（重試 {} 次、超過 {} 秒仍未成功，已中止）",
                self.attempts,
                self.limit.as_secs()
            ));
        }
        gateway_error
    }
}

/// 純文字退路，只用於沒有結構化事件可讀的路徑。回 `None` 表示看不出上游錯誤。
///
/// 樣式刻意保持嚴格，取捨與理由：
///
/// - **不採寬鬆的 `410` / `429` 裸數字比對。** 加了 `--print-logs` 之後整包 request
///   body 會原樣進 stderr，跑市場分析的排程出現「成交量 429 億美元」完全正常，寬鬆
///   比對會砍掉一個本來會成功的任務。誤殺比漏判更糟——漏判是靜默，誤殺是中斷服務。
/// - **不認裸的 `Gone`。** 上游 handover 的樣式表列了它，這裡刻意排除：一句「the
///   opportunity is gone」就會誤觸，與上一條是同一類的誤判。要判 410 就去比對
///   `statusCode`。
/// - **不認 404。** 理由同 [`classify`]。
pub fn classify_text(text: &str) -> Option<UpstreamFailure> {
    if has_status_signal(text, 410) || contains_any(text, &["end of life"]) {
        return Some(UpstreamFailure::ModelEol);
    }
    if has_status_signal(text, 429)
        || contains_any(text, &["RESOURCE_EXHAUSTED"])
        || contains_any_ignore_ascii_case(text, &["usage limit reached"])
    {
        return Some(UpstreamFailure::RateLimited);
    }
    if contains_any(text, &["ProviderModelNotFoundError", "Model not found"]) {
        return Some(UpstreamFailure::ModelEol);
    }
    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn contains_any_ignore_ascii_case(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    needles
        .iter()
        .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
}

/// 只在數字明確作為 `status` / `statusCode` 的**值**出現時才算命中。涵蓋
/// `"statusCode":410`、`"status": 410`、`statusCode=410` 與 `statusCode 410`。
fn has_status_signal(text: &str, code: u16) -> bool {
    let code = code.to_string();
    for key in ["\"statusCode\"", "\"status\"", "statusCode", "status"] {
        let mut rest = text;
        while let Some(pos) = rest.find(key) {
            let after = &rest[pos + key.len()..];
            if separator_then_code(after, &code) {
                return true;
            }
            rest = &rest[pos + key.len()..];
        }
    }
    false
}

fn separator_then_code(after: &str, code: &str) -> bool {
    let had_space = after.starts_with([' ', '\t']);
    let trimmed = after.trim_start_matches([' ', '\t']);
    let trimmed = match trimmed
        .strip_prefix(':')
        .or_else(|| trimmed.strip_prefix('='))
    {
        Some(rest) => rest.trim_start_matches([' ', '\t']),
        None if had_space => trimmed,
        None => return false,
    };
    // JSON 可能把數字包成字串。
    let trimmed = trimmed.strip_prefix('"').unwrap_or(trimmed);
    match trimmed.strip_prefix(code) {
        // 後面不能再接數字，否則 4100 會被當成 410。
        Some(rest) => !rest.starts_with(|c: char| c.is_ascii_digit()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_status_code_decides_first() {
        assert_eq!(classify("APIError", Some(410)), UpstreamFailure::ModelEol);
        assert_eq!(
            classify("APIError", Some(429)),
            UpstreamFailure::RateLimited
        );
    }

    #[test]
    fn aborted_is_distinguishable_from_model_eol() {
        // 告警要說得出原因：中止不是下架。
        assert_eq!(
            classify("MessageAbortedError", None),
            UpstreamFailure::Aborted
        );
        assert_ne!(
            classify("MessageAbortedError", None),
            UpstreamFailure::ModelEol
        );
    }

    #[test]
    fn does_not_treat_404_as_model_eol() {
        // 非對話類模型打錯端點也回 404，混進來會讓告警說錯原因。
        assert_eq!(classify("APIError", Some(404)), UpstreamFailure::Other);
        assert_ne!(classify("APIError", Some(404)), UpstreamFailure::ModelEol);
    }

    #[test]
    fn unknown_named_error_is_other() {
        assert_eq!(classify("ContentFilterError", None), UpstreamFailure::Other);
        assert_eq!(classify("APIError", Some(500)), UpstreamFailure::Other);
    }

    #[test]
    fn text_fallback_matches_json_and_kv_forms() {
        assert_eq!(
            classify_text(r#"{"statusCode":410,"message":"gone"}"#),
            Some(UpstreamFailure::ModelEol)
        );
        assert_eq!(
            classify_text(r#"{"status": "429"}"#),
            Some(UpstreamFailure::RateLimited)
        );
        assert_eq!(
            classify_text("statusCode=410 end of life"),
            Some(UpstreamFailure::ModelEol)
        );
        assert_eq!(
            classify_text("statusCode 429"),
            Some(UpstreamFailure::RateLimited)
        );
        assert_eq!(
            classify_text("this model reached end of life"),
            Some(UpstreamFailure::ModelEol)
        );
        assert_eq!(
            classify_text("RESOURCE_EXHAUSTED"),
            Some(UpstreamFailure::RateLimited)
        );
        assert_eq!(
            classify_text(
                "AI_APICallError: Free usage limit reached. Free models work in OpenCode"
            ),
            Some(UpstreamFailure::RateLimited)
        );
        assert_eq!(
            classify_text("error: USAGE LIMIT REACHED FOR CURRENT TIER"),
            Some(UpstreamFailure::RateLimited)
        );
    }

    #[test]
    fn text_fallback_does_not_misjudge_ordinary_output() {
        // 這是本模組存在的理由之一：跑市場分析的排程出現這種句子完全正常，
        // 寬鬆的裸數字比對會砍掉一個本來會成功的任務。
        assert_eq!(
            classify_text("本季成交量 410 億美元，較上季 429 億美元下滑"),
            None
        );
        assert_eq!(
            classify_text("Revenue was 410 million, up from 429 last year"),
            None
        );
        // 裸的 Gone 不算——「the opportunity is gone」不是模型下架。
        assert_eq!(classify_text("the opportunity is gone"), None);
        // 4100 不得被當成 410。
        assert_eq!(classify_text(r#"{"statusCode":4100}"#), None);
        assert_eq!(classify_text(""), None);
    }

    #[test]
    fn parses_the_shape_observed_from_both_paths() {
        // Server SSE：session.error 的 properties（2026-09-08 abort 實測原文）
        let sse: serde_json::Value = serde_json::from_str(
            r#"{"sessionID":"ses_f7fd5b167ffehvG8KZb3Z0xZ84","error":{"name":"MessageAbortedError","data":{"message":"Aborted"}}}"#,
        )
        .unwrap();
        let parsed = parse_error_field(&sse).unwrap();
        assert_eq!(parsed.name, "MessageAbortedError");
        assert_eq!(parsed.status_code, None);
        assert_eq!(parsed.message, "Aborted");

        // CLI --format json：{"type":"error", ...} 的根物件（同日實測原文）
        let cli: serde_json::Value = serde_json::from_str(
            r#"{"type":"error","timestamp":1788857671885,"sessionID":"ses_x","error":{"name":"UnknownError","data":{"message":"Unexpected server error.","ref":"err_0051c87d"}}}"#,
        )
        .unwrap();
        let parsed = parse_error_field(&cli).unwrap();
        assert_eq!(parsed.name, "UnknownError");
        assert_eq!(parsed.status_code, None);

        // APIError 帶 statusCode——這是主判定依據。
        let api: serde_json::Value = serde_json::from_str(
            r#"{"error":{"name":"APIError","data":{"message":"gone","statusCode":410,"isRetryable":false}}}"#,
        )
        .unwrap();
        let parsed = parse_error_field(&api).unwrap();
        assert_eq!(parsed.status_code, Some(410));
        assert_eq!(
            classify(&parsed.name, parsed.status_code),
            UpstreamFailure::ModelEol
        );

        // 沒有 error 欄位就不是錯誤事件。
        assert!(parse_error_field(&serde_json::json!({"type": "text"})).is_none());
    }

    #[test]
    fn labels_are_distinct_and_non_empty() {
        let all = [
            UpstreamFailure::ModelEol,
            UpstreamFailure::RateLimited,
            UpstreamFailure::Aborted,
            UpstreamFailure::Unreachable,
            UpstreamFailure::Other,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(!a.label().is_empty());
            assert_eq!(a.to_string(), a.label());
            for b in &all[i + 1..] {
                assert_ne!(a.label(), b.label());
            }
        }
    }

    // issue-0044 SCN-007：V2 的錯誤類型照原本的分類。
    #[test]
    fn v2_error_types_keep_their_categories() {
        for limit in [
            "provider.rate-limit",
            "provider.quota",
            "provider.free-limit",
            "provider.go-limit",
            "provider.budget-limit",
        ] {
            assert_eq!(
                classify(limit, None),
                UpstreamFailure::RateLimited,
                "{limit}"
            );
        }
        assert_eq!(
            classify("provider.no-route", None),
            UpstreamFailure::ModelEol
        );
        for unreachable in ["provider.transport", "provider.connect", "provider.timeout"] {
            assert_eq!(
                classify(unreachable, None),
                UpstreamFailure::Unreachable,
                "{unreachable}"
            );
        }
        // 狀態碼仍然先判：上游回 410 時不論類型都是下架。
        assert_eq!(
            classify("provider.invalid-request", Some(410)),
            UpstreamFailure::ModelEol
        );
        assert_eq!(classify("provider.auth", None), UpstreamFailure::Other);
    }

    fn fixture_line(name: &str, event_type: &str) -> serde_json::Value {
        let path = format!(
            "{}/tests/fixtures/opencode-v2/{name}.ndjson",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value["type"] == event_type)
            .unwrap()
    }

    #[test]
    fn parses_the_v2_cli_error_line() {
        let line = fixture_line("cli-no-route", "error");
        let error = parse_error_field(&line).unwrap();
        assert_eq!(error.name, "provider.no-route");
        assert_eq!(error.message, "Model unavailable: missing/nope");
        assert_eq!(
            classify(&error.name, error.status_code),
            UpstreamFailure::ModelEol
        );
    }

    // issue-0044 SCN-007 第二列：模型下架的真實樣本（2026-11-25，NVIDIA 回 410 end of life）。
    // V2 的錯誤類型是通用的 provider.invalid-request，靠 status 410 才判得出下架。
    #[test]
    fn a_real_end_of_life_410_is_model_eol() {
        let line = fixture_line("cli-model-eol", "error");
        let error = parse_error_field(&line).unwrap();
        assert_eq!(error.name, "provider.invalid-request");
        assert_eq!(error.status_code, Some(410));
        assert!(error.message.contains("end of life"), "{}", error.message);
        assert_eq!(
            classify(&error.name, error.status_code),
            UpstreamFailure::ModelEol
        );
    }

    fn transport_error() -> Option<UpstreamError> {
        parse_error_field(&fixture_line("cli-retry-transport", "retry"))
    }

    // issue-0044 SCN-007 第三列：重試持續超過時限就以重試的錯誤類型中止。
    #[tokio::test(start_paused = true)]
    async fn retries_that_last_too_long_give_up_as_unreachable() {
        let mut watch = RetryWatch::default();
        assert!(watch.deadline().is_none());
        let started = tokio::time::Instant::now();
        watch.retrying(transport_error());
        tokio::time::sleep(std::time::Duration::from_secs(40)).await;
        watch.retrying(transport_error());

        watch.expired().await;

        assert_eq!(started.elapsed(), RETRY_GIVE_UP_AFTER);
        match watch.give_up() {
            crate::error::GatewayError::UpstreamFailed { kind, detail, .. } => {
                assert_eq!(kind, UpstreamFailure::Unreachable);
                assert!(detail.contains("Can't reach opencode.ai"), "{detail}");
                assert!(detail.contains("重試 2 次"), "{detail}");
            }
            other => panic!("expected UpstreamFailed, got {other:?}"),
        }
    }

    // issue-0044 SCN-016：時限內恢復就重新起算，不會被中止。
    #[tokio::test(start_paused = true)]
    async fn a_recovery_restarts_the_clock() {
        let mut watch = RetryWatch::default();
        watch.retrying(transport_error());
        tokio::time::sleep(std::time::Duration::from_secs(80)).await;
        watch.recovered();
        assert!(watch.deadline().is_none());

        watch.retrying(transport_error());
        let expires_at = watch.deadline().unwrap();
        assert_eq!(
            expires_at,
            tokio::time::Instant::now() + RETRY_GIVE_UP_AFTER
        );
    }

    #[tokio::test(start_paused = true)]
    async fn not_retrying_never_expires() {
        let watch = RetryWatch::default();
        let waited =
            tokio::time::timeout(std::time::Duration::from_secs(3600), watch.expired()).await;
        assert!(waited.is_err());
    }
}
