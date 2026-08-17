use thiserror::Error;

/// Errors from the Telegram transport layer.
///
/// `Http` 刻意存**已遮蔽的字串**而不是 `reqwest::Error`。Telegram 把 bot token 放在
/// request path 裡（`/bot<token>/sendMessage`），而 `reqwest::Error` 的 Display 與
/// Debug 都會把整個 URL 印出來——只要在呼叫端 `{e}` 一次就把 token 寫進 stderr，
/// 而那正是 `log_send`（12 處）與 schedulerd 的 delivery 警告在做的事。
///
/// 在型別裡就地遮蔽，而不是去修每個 log 呼叫點：呼叫點會再長出第 13 個，型別不會。
/// 代價是失去 `source()` 的 typed 存取，但全 repo 沒有任何地方 match
/// `TgError::Http` 的 payload 或用到 reqwest 的 `is_timeout()` 之類。
#[derive(Debug, Error)]
pub enum TgError {
    #[error("http error: {0}")]
    Http(String),
    #[error("telegram api error: {0}")]
    Api(String),
}

impl From<reqwest::Error> for TgError {
    fn from(err: reqwest::Error) -> Self {
        TgError::Http(redact_bot_token(&flatten_sources(&err)))
    }
}

/// 把 URL 路徑裡的 bot token 換成 `<redacted>`。
///
/// 同時涵蓋 API base（`/bot<token>`）與 file base（`/file/bot<token>`）。只在該段
/// 看起來真的是 token（`<數字>:<秘密>`，含 `:`）時才動手，免得把 `/bots/list`
/// 這類無關路徑一起改掉。
fn redact_bot_token(rendered: &str) -> String {
    let mut out = String::with_capacity(rendered.len());
    let mut rest = rendered;
    while let Some(idx) = rest.find("/bot") {
        let (before, after) = rest.split_at(idx + "/bot".len());
        out.push_str(before);
        // token 字元集是 alnum 加 `:`、`_`、`-`；碰到 `/`、`)`、空白等就是這段結束。
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-')))
            .unwrap_or(after.len());
        if after[..end].contains(':') {
            out.push_str("<redacted>");
            rest = &after[end..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// 把 error 的 source 鏈攤平成一行。
///
/// `reqwest::Error` 的 Display 只說「error sending request for url (...)」——真正的
/// 原因（connection refused、timeout）躺在 source 鏈裡。既然這裡本來就要重新渲染，
/// 順手把原因帶上：遮蔽前的日誌其實既洩漏 token 又沒說為什麼失敗。
fn flatten_sources(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut src = err.source();
    while let Some(e) = src {
        let next = e.to_string();
        if !out.contains(&next) {
            out.push_str(": ");
            out.push_str(&next);
        }
        src = e.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_TOKEN: &str = "7654321:AAFAKE_TOKEN_VALUE";

    fn probe_url() -> String {
        // 127.0.0.1:1 必定 connection refused，測試不需要網路。
        format!("http://127.0.0.1:1/bot{FAKE_TOKEN}/sendMessage")
    }

    /// 用**真的** reqwest 失敗來測，不用字串 fixture：會不會洩漏取決於當下 reqwest
    /// 版本怎麼渲染 URL，那是升級會變的東西，fixture 驗不到。
    #[tokio::test]
    async fn transport_failure_never_renders_the_bot_token() {
        let raw = reqwest::Client::new()
            .get(probe_url())
            .send()
            .await
            .unwrap_err();

        // 前提檢查：reqwest 若哪天不再把 URL 放進 Display，這個測試就失去鑑別力，
        // 必須紅燈提醒而不是靜靜通過。
        assert!(
            format!("{raw}").contains(FAKE_TOKEN),
            "前提失效：reqwest 已不再於 Display 帶出 URL，本測試不再能證明遮蔽有效"
        );

        let err = TgError::from(raw);
        assert!(
            !format!("{err}").contains(FAKE_TOKEN),
            "Display 洩漏 token：{err}"
        );
        assert!(
            !format!("{err:?}").contains(FAKE_TOKEN),
            "Debug 洩漏 token：{err:?}"
        );

        let mut src = std::error::Error::source(&err);
        while let Some(e) = src {
            assert!(
                !format!("{e}").contains(FAKE_TOKEN),
                "source 鏈洩漏 token：{e}"
            );
            src = e.source();
        }
    }

    /// 遮蔽不能順手把失敗原因也吃掉——不然日誌從「洩漏 token」變成「什麼都沒說」。
    #[tokio::test]
    async fn transport_failure_keeps_the_underlying_reason() {
        let raw = reqwest::Client::new()
            .get(probe_url())
            .send()
            .await
            .unwrap_err();
        let rendered = format!("{}", TgError::from(raw)).to_ascii_lowercase();
        assert!(
            rendered.contains("refused") || rendered.contains("connect"),
            "攤平 source 鏈是為了保留真正原因，但渲染結果看不到：{rendered}"
        );
    }

    #[test]
    fn redacts_api_and_file_urls_but_leaves_unrelated_paths_alone() {
        let s = redact_bot_token(
            "https://api.telegram.org/bot123:SECRET/sendMessage \
             https://api.telegram.org/file/bot123:SECRET/photo.jpg \
             https://example.com/bots/list",
        );
        assert!(!s.contains("SECRET"), "{s}");
        assert_eq!(s.matches("/bot<redacted>").count(), 2, "{s}");
        assert!(s.contains("/bots/list"), "誤傷無關路徑：{s}");
    }

    /// token 在字串結尾、後面沒有路徑分隔時也要遮乾淨（base URL 本身出錯的情況）。
    #[test]
    fn redacts_a_token_that_ends_the_string_or_is_followed_by_punctuation() {
        assert_eq!(
            redact_bot_token("url (https://api.telegram.org/bot123:SECRET)"),
            "url (https://api.telegram.org/bot<redacted>)"
        );
        assert_eq!(
            redact_bot_token("https://api.telegram.org/bot123:SECRET"),
            "https://api.telegram.org/bot<redacted>"
        );
    }
}
