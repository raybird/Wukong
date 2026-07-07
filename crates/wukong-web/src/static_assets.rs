//! Embedded browser assets (HTML/JS/CSS) and their trivial static handlers.
//!
//! The `include_str!` paths are relative to this file; since it lives in `src/`
//! alongside `lib.rs`, the `../static/...` paths resolve identically.

pub(crate) const INDEX_HTML: &str = include_str!("../static/index.html");

pub(crate) const APP_JS: &str = include_str!("../static/app.js");
pub(crate) const HTML_JS: &str = include_str!("../static/lib/html.js");
pub(crate) const CHAT_LAYOUT_JS: &str = include_str!("../static/lib/chat-layout.mjs");
pub(crate) const UNREAD_MARKER_JS: &str = include_str!("../static/lib/unread-marker.mjs");
pub(crate) const CHAT_JS: &str = include_str!("../static/components/wukong-chat.js");
pub(crate) const CHAT_THREAD_HEADER_JS: &str =
    include_str!("../static/components/chat-thread-header.js");
pub(crate) const CHAT_MESSAGE_JS: &str = include_str!("../static/components/chat-message.js");
pub(crate) const CHAT_ACTIVITY_JS: &str = include_str!("../static/components/chat-activity.js");
pub(crate) const CHAT_QUESTION_CARD_JS: &str =
    include_str!("../static/components/chat-question-card.js");
pub(crate) const MEMORY_JS: &str = include_str!("../static/components/wukong-memory.js");
pub(crate) const SKILLS_JS: &str = include_str!("../static/components/wukong-skills.js");
pub(crate) const SETTINGS_JS: &str = include_str!("../static/components/wukong-settings.js");
pub(crate) const SCHEDULES_JS: &str = include_str!("../static/components/wukong-schedules.js");
pub(crate) const SYSTEM_JS: &str = include_str!("../static/components/wukong-system.js");
pub(crate) const STYLES_CSS: &str = include_str!("../static/styles.css");

const JS: &str = "application/javascript";
const CSS: &str = "text/css";

/// Build a static-asset response with an explicit content type.
fn asset(content_type: &'static str, body: &'static str) -> axum::response::Response {
    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

pub(crate) async fn app_js() -> axum::response::Response {
    asset(JS, APP_JS)
}
pub(crate) async fn html_js() -> axum::response::Response {
    asset(JS, HTML_JS)
}
pub(crate) async fn chat_layout_js() -> axum::response::Response {
    asset(JS, CHAT_LAYOUT_JS)
}
pub(crate) async fn unread_marker_js() -> axum::response::Response {
    asset(JS, UNREAD_MARKER_JS)
}
pub(crate) async fn chat_js() -> axum::response::Response {
    asset(JS, CHAT_JS)
}
pub(crate) async fn chat_thread_header_js() -> axum::response::Response {
    asset(JS, CHAT_THREAD_HEADER_JS)
}
pub(crate) async fn chat_message_js() -> axum::response::Response {
    asset(JS, CHAT_MESSAGE_JS)
}
pub(crate) async fn chat_activity_js() -> axum::response::Response {
    asset(JS, CHAT_ACTIVITY_JS)
}
pub(crate) async fn chat_question_card_js() -> axum::response::Response {
    asset(JS, CHAT_QUESTION_CARD_JS)
}
pub(crate) async fn memory_js() -> axum::response::Response {
    asset(JS, MEMORY_JS)
}
pub(crate) async fn skills_js() -> axum::response::Response {
    asset(JS, SKILLS_JS)
}
pub(crate) async fn settings_js() -> axum::response::Response {
    asset(JS, SETTINGS_JS)
}
pub(crate) async fn schedules_js() -> axum::response::Response {
    asset(JS, SCHEDULES_JS)
}
pub(crate) async fn system_js() -> axum::response::Response {
    asset(JS, SYSTEM_JS)
}
pub(crate) async fn styles_css() -> axum::response::Response {
    asset(CSS, STYLES_CSS)
}
