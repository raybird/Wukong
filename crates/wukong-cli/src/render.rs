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
                let _ = writeln!(self.err, "  💭 {t}");
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
