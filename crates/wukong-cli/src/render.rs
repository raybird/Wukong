//! Render StreamEvents to a terminal: assistant text to stdout, activity
//! (tools, spinner cues) to stderr. Writers are injected for testability.

use std::io::Write;
use wukong_gateway::StreamEvent;

/// Routes streamed events to two writers. `out` receives assistant text
/// (pipe-friendly); `err` receives activity lines (tools, etc.).
pub struct StreamRenderer<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl<'a> StreamRenderer<'a> {
    pub fn new(out: &'a mut dyn Write, err: &'a mut dyn Write) -> Self {
        Self { out, err }
    }

    /// Handle one event. Text → out; ToolUse → err; step events are spinner
    /// cues handled by the live UI (no-op for buffered writers here).
    pub fn on_event(&mut self, ev: &StreamEvent) {
        match ev {
            StreamEvent::Text(t) => {
                let _ = write!(self.out, "{t}");
                let _ = self.out.flush();
            }
            StreamEvent::ToolUse(name) => {
                let _ = writeln!(self.err, "  ▸ 使用工具 {name}");
                let _ = self.err.flush();
            }
            StreamEvent::Reasoning(t) => {
                if !t.trim().is_empty() {
                    let _ = writeln!(self.err, "  💭 {t}");
                    let _ = self.err.flush();
                }
            }
            // CLI 尚無互動回覆通道；至少要讓詢問可見，否則使用者只會看到
            // 畫面停住，直到 stream deadline 才知道發生什麼事。
            StreamEvent::QuestionRequest(request) => {
                for question in &request.questions {
                    let _ = writeln!(self.err, "  {}", question.header);
                    for line in question.question.trim().lines() {
                        let _ = writeln!(self.err, "     {line}");
                    }
                }
                let _ = writeln!(
                    self.err,
                    "     ↳ CLI 無法回覆此詢問，請改用 Web Console 或 Telegram，否則本回合會等到逾時。"
                );
                let _ = self.err.flush();
            }
            StreamEvent::StepStart | StreamEvent::StepFinish => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_request_is_surfaced_to_activity_output() {
        use wukong_gateway::stream::{QuestionInfo, QuestionRequest};

        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        {
            let mut r = StreamRenderer::new(&mut out, &mut err);
            r.on_event(&StreamEvent::QuestionRequest(QuestionRequest {
                request_id: "permission-per_1".to_string(),
                session_id: "ses_1".to_string(),
                questions: vec![QuestionInfo {
                    question: "OpenCode 要求執行權限：external_directory\n\n範圍：\n• /tmp/*"
                        .to_string(),
                    header: "🔐 權限確認".to_string(),
                    options: Vec::new(),
                    multiple: false,
                    custom: false,
                }],
            }));
        }
        let err_s = String::from_utf8(err).unwrap();
        assert!(err_s.contains("🔐 權限確認"), "{err_s}");
        assert!(err_s.contains("external_directory"), "{err_s}");
        assert!(err_s.contains("/tmp/*"), "{err_s}");
        assert!(err_s.contains("CLI 無法回覆"), "{err_s}");
        // 詢問屬於活動訊息，不能混進 stdout 的助理輸出。
        assert!(out.is_empty());
    }

    #[test]
    fn reasoning_shows_only_when_nonempty() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        {
            let mut r = StreamRenderer::new(&mut out, &mut err);
            r.on_event(&StreamEvent::Reasoning("".to_string()));
            r.on_event(&StreamEvent::Reasoning("想一下".to_string()));
        }
        let err_s = String::from_utf8(err).unwrap();
        assert!(err_s.contains("💭"));
        assert!(err_s.contains("想一下"));
        // Exactly one reasoning line (the empty one produced nothing).
        assert_eq!(err_s.matches("💭").count(), 1);
    }

    #[test]
    fn text_goes_to_out_tools_to_err() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        {
            let mut r = StreamRenderer::new(&mut out, &mut err);
            r.on_event(&StreamEvent::StepStart);
            r.on_event(&StreamEvent::ToolUse("read".to_string()));
            r.on_event(&StreamEvent::Text("hello ".to_string()));
            r.on_event(&StreamEvent::Text("world".to_string()));
            r.on_event(&StreamEvent::StepFinish);
        }
        assert_eq!(String::from_utf8(out).unwrap(), "hello world");
        assert_eq!(String::from_utf8(err).unwrap(), "  ▸ 使用工具 read\n");
    }
}
