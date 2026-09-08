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
pub fn parse_error_field(container: &serde_json::Value) -> Option<UpstreamError> {
    use serde_json::Value;
    let error = container.get("error")?;
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

/// 上游錯誤的分類。決定告警怎麼說、以及是否值得重試。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamFailure {
    /// 模型已下架（410）。換模型才會好，重試無用。
    ModelEol,
    /// 上游限流（429）。稍後重試可能會好。
    RateLimited,
    /// 回合被中止。不是上游的錯，但也**不是成功**。
    Aborted,
    /// 其他上游錯誤。
    Other,
}

impl UpstreamFailure {
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelEol => "模型已下架",
            Self::RateLimited => "上游限流",
            Self::Aborted => "回合被中止",
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
        "MessageAbortedError" => UpstreamFailure::Aborted,
        "ProviderModelNotFoundError" => UpstreamFailure::ModelEol,
        _ => UpstreamFailure::Other,
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
    if has_status_signal(text, 429) || contains_any(text, &["RESOURCE_EXHAUSTED"]) {
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
}
