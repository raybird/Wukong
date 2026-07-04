# Opencode-Inspired Web Console Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign Wukong Web Console chat into an opencode-inspired workbench while keeping Wukong's current backend APIs and chat history ownership.

**Architecture:** Add small browser rendering modules for thread header, message frames, activity cards, and question cards, then refactor `WukongChat` into the coordinator for data loading and live events. Serve the new modules through existing `include_str!` static routes and apply workbench/activity-rail CSS without changing backend schemas or opencode server ownership.

**Tech Stack:** Plain browser ES modules, custom elements, Axum static asset routes, existing Rust web tests, Node `--check`, existing `unread-marker.mjs` helper.

---

## File Structure

- Create: `crates/wukong-web/static/components/chat-thread-header.js`
  - Pure DOM helper for the chat workbench header shell. It renders the scope selector, jump controls, model status, skill status, and static status labels.
- Create: `crates/wukong-web/static/components/chat-message.js`
  - Pure DOM helpers for message frames, date separators, unread divider, attachments, and generic bubbles/status frames.
- Create: `crates/wukong-web/static/components/chat-activity.js`
  - Pure DOM helpers for lazy events, lazy helper steps, live thinking details, live progress bubbles, and activity rail markers.
- Create: `crates/wukong-web/static/components/chat-question-card.js`
  - Pure DOM helper for OpenCode question cards. It accepts callbacks for submit/reject so it does not know API routes.
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
  - Imports the helpers above, coordinates API calls, state, event streams, unread behavior, and composer behavior.
- Modify: `crates/wukong-web/static/styles.css`
  - Adds workbench layout, message frame, activity rail, responsive header, and updated question-card styles.
- Modify: `crates/wukong-web/src/lib.rs`
  - Serves the new static component modules and extends static/string tests.

## Task 1: Add Rendering Modules And Static Routes

**Files:**
- Create: `crates/wukong-web/static/components/chat-thread-header.js`
- Create: `crates/wukong-web/static/components/chat-message.js`
- Create: `crates/wukong-web/static/components/chat-activity.js`
- Create: `crates/wukong-web/static/components/chat-question-card.js`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing static serving**

Run:

```text
gitnexus_impact({
  target: "build_router",
  direction: "upstream",
  file_path: "crates/wukong-web/src/lib.rs",
  repo: "Wukong",
  maxDepth: 2
})
```

Expected: likely HIGH or CRITICAL because route tests all construct the router. Continue only after noting that this task adds static asset routes and is covered by `serves_static_assets_with_content_types`.

- [ ] **Step 2: Add failing static asset assertions**

In `crates/wukong-web/src/lib.rs`, update `serves_static_assets_with_content_types()` by adding these assertions after the existing `/components/wukong-chat.js` assertion:

```rust
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-thread-header.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-message.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-activity.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-question-card.js"
        )
        .await
        .contains("javascript"));
```

- [ ] **Step 3: Run the route test and verify failure**

Run:

```bash
cargo test -p wukong-web serves_static_assets_with_content_types
```

Expected: FAIL with one of the new component paths returning `404`.

- [ ] **Step 4: Add minimal helper modules**

Create `crates/wukong-web/static/components/chat-thread-header.js`:

```js
import { html } from '/lib/html.js';

export function threadHeaderTemplate() {
  return html`
    <section class="chat-thread-header" aria-label="對話工作台狀態">
      <div class="thread-source">
        <label class="chat-source">來源 <select id="chat-scope"></select></label>
      </div>
      <div class="thread-jump">
        <label>跳到日期 <input id="jump-date" type="date" /></label>
        <button id="jump-button" type="button">前往</button>
      </div>
      <div class="thread-status" aria-live="polite">
        <span id="chat-model-status" class="tag">模型：載入中</span>
        <span id="chat-skill-status" class="tag">技能偏好：Phase 2</span>
      </div>
    </section>
  `.toString();
}
```

Create `crates/wukong-web/static/components/chat-message.js`:

```js
import { escapeHTML } from '/lib/html.js';

export function dateSeparatorNode(text) {
  const sep = document.createElement('div');
  sep.className = 'date-separator';
  sep.textContent = text;
  return sep;
}

export function unreadDividerNode() {
  const div = document.createElement('div');
  div.className = 'unread-divider';
  div.textContent = '以下是上次離開後的新紀錄';
  return div;
}

export function messageFrameNode(message, { attachmentsNode } = {}) {
  const frame = document.createElement('article');
  const role = message.role === 'user' ? 'user' : 'assistant';
  frame.className = 'message-frame ' + role;
  frame.dataset.messageId = message.id;
  frame.dataset.role = role;

  const meta = document.createElement('header');
  meta.className = 'message-meta';
  const label = role === 'user' ? '你' : '悟空';
  meta.innerHTML = '<span>' + escapeHTML(label) + '</span>';
  frame.appendChild(meta);

  const body = document.createElement('div');
  body.className = 'message-content bubble ' + role;
  if (message.role === 'assistant' && message.content_html) body.innerHTML = message.content_html;
  else body.textContent = message.content || '';
  if (message.status === 'error') body.classList.add('error');
  frame.appendChild(body);

  const attachments = attachmentsNode ? attachmentsNode(message) : null;
  if (attachments) frame.appendChild(attachments);
  return { frame, body };
}

export function bubbleNode(cls, innerHTML) {
  const div = document.createElement('div');
  div.className = 'bubble ' + cls;
  div.innerHTML = innerHTML;
  return div;
}
```

Create `crates/wukong-web/static/components/chat-activity.js`:

```js
import { escapeHTML } from '/lib/html.js';

export function activityDetailsNode({ className, summary, loadingText }) {
  const details = document.createElement('details');
  details.className = 'activity-card ' + className;
  details.innerHTML = '<summary>' + escapeHTML(summary) + '</summary><div class="activity-card-body"></div>';
  details.querySelector('.activity-card-body').innerHTML = '<p class="baton-loading">' + escapeHTML(loadingText) + '</p>';
  return details;
}

export function liveThinkingNode() {
  const details = document.createElement('details');
  details.className = 'activity-card thinking';
  details.open = true;
  details.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
  return details;
}

export function activityRailNode() {
  const rail = document.createElement('div');
  rail.className = 'activity-rail';
  return rail;
}
```

Create `crates/wukong-web/static/components/chat-question-card.js`:

```js
import { escapeHTML } from '/lib/html.js';

export function questionCardNode(request, source, { onSubmit, onReject }) {
  if (!request || !request.request_id || !request.session_id || !Array.isArray(request.questions)) return null;
  const state = {
    tab: 0,
    answers: request.questions.map(() => []),
    custom: request.questions.map(() => ''),
    sending: false,
  };
  const card = document.createElement('section');
  card.className = 'question-card activity-card';
  card.dataset.requestId = request.request_id;
  card.dataset.source = source || '';

  const finish = (text) => {
    card.classList.add('question-card-done');
    card.innerHTML = '<div class="question-done">' + escapeHTML(text) + '</div>';
  };

  const setStatus = (text, cls = '') => {
    const status = card.querySelector('.question-status');
    if (!status) return;
    status.textContent = text;
    status.className = 'question-status ' + cls;
  };

  const submit = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('送出中…');
    try {
      await onSubmit(request, state.answers);
      finish('已送出回答。');
    } catch (err) {
      state.sending = false;
      setStatus('送出失敗：' + err.message, 'error');
    }
  };

  const reject = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('取消中…');
    try {
      await onReject(request);
      finish('已取消問題。');
    } catch (err) {
      state.sending = false;
      setStatus('取消失敗：' + err.message, 'error');
    }
  };

  const render = () => {
    const question = request.questions[state.tab];
    if (!question) return;
    const selected = state.answers[state.tab] || [];
    const isLast = state.tab >= request.questions.length - 1;
    card.innerHTML = '<div class="question-title">問題 ' + (state.tab + 1) + ' / ' + request.questions.length + '</div>';
    if (question.header) card.innerHTML += '<div class="question-header">' + escapeHTML(question.header) + '</div>';
    card.innerHTML += '<div class="question-text">' + escapeHTML(question.question || '') + '</div>';
    const options = document.createElement('div');
    options.className = 'question-options';
    for (const option of question.options || []) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'question-option';
      const picked = selected.includes(option.label);
      if (picked) button.classList.add('picked');
      button.innerHTML = '<span>' + escapeHTML(picked ? '✓ ' : '') + escapeHTML(option.label || '') + '</span>' +
        (option.description ? '<small>' + escapeHTML(option.description) + '</small>' : '');
      button.addEventListener('click', () => {
        if (question.multiple) {
          state.answers[state.tab] = picked
            ? selected.filter((item) => item !== option.label)
            : [...selected, option.label];
          render();
          return;
        }
        state.answers[state.tab] = [option.label];
        if (isLast) void submit();
        else {
          state.tab += 1;
          render();
        }
      });
      options.appendChild(button);
    }
    card.appendChild(options);
    if (question.custom) {
      const custom = document.createElement('textarea');
      custom.className = 'question-custom';
      custom.rows = 2;
      custom.placeholder = '自訂回答…';
      custom.value = state.custom[state.tab] || '';
      custom.addEventListener('input', () => {
        state.custom[state.tab] = custom.value;
      });
      card.appendChild(custom);
    }
    const status = document.createElement('div');
    status.className = 'question-status';
    card.appendChild(status);
    const footer = document.createElement('div');
    footer.className = 'question-footer';
    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = '取消';
    cancel.addEventListener('click', () => void reject());
    footer.appendChild(cancel);
    if (!isLast) {
      const next = document.createElement('button');
      next.type = 'button';
      next.textContent = '下一題';
      next.addEventListener('click', () => {
        const custom = (state.custom[state.tab] || '').trim();
        if (custom) {
          state.answers[state.tab] = question.multiple
            ? Array.from(new Set([...(state.answers[state.tab] || []), custom]))
            : [custom];
        }
        state.tab += 1;
        render();
      });
      footer.appendChild(next);
    } else {
      const send = document.createElement('button');
      send.type = 'button';
      send.textContent = '送出';
      send.addEventListener('click', () => {
        const custom = (state.custom[state.tab] || '').trim();
        if (custom) {
          state.answers[state.tab] = question.multiple
            ? Array.from(new Set([...(state.answers[state.tab] || []), custom]))
            : [custom];
        }
        void submit();
      });
      footer.appendChild(send);
    }
    card.appendChild(footer);
  };

  render();
  return card;
}
```

- [ ] **Step 5: Add static includes, handlers, and routes**

In `crates/wukong-web/src/lib.rs`, add constants next to existing component constants:

```rust
const CHAT_THREAD_HEADER_JS: &str = include_str!("../static/components/chat-thread-header.js");
const CHAT_MESSAGE_JS: &str = include_str!("../static/components/chat-message.js");
const CHAT_ACTIVITY_JS: &str = include_str!("../static/components/chat-activity.js");
const CHAT_QUESTION_CARD_JS: &str = include_str!("../static/components/chat-question-card.js");
```

Add handlers next to `chat_js()`:

```rust
async fn chat_thread_header_js() -> axum::response::Response {
    asset(JS, CHAT_THREAD_HEADER_JS)
}
async fn chat_message_js() -> axum::response::Response {
    asset(JS, CHAT_MESSAGE_JS)
}
async fn chat_activity_js() -> axum::response::Response {
    asset(JS, CHAT_ACTIVITY_JS)
}
async fn chat_question_card_js() -> axum::response::Response {
    asset(JS, CHAT_QUESTION_CARD_JS)
}
```

Add routes after `/components/wukong-chat.js`:

```rust
        .route(
            "/components/chat-thread-header.js",
            axum::routing::get(chat_thread_header_js),
        )
        .route(
            "/components/chat-message.js",
            axum::routing::get(chat_message_js),
        )
        .route(
            "/components/chat-activity.js",
            axum::routing::get(chat_activity_js),
        )
        .route(
            "/components/chat-question-card.js",
            axum::routing::get(chat_question_card_js),
        )
```

- [ ] **Step 6: Run syntax and route checks**

Run:

```bash
node --check crates/wukong-web/static/components/chat-thread-header.js
node --check crates/wukong-web/static/components/chat-message.js
node --check crates/wukong-web/static/components/chat-activity.js
node --check crates/wukong-web/static/components/chat-question-card.js
cargo test -p wukong-web serves_static_assets_with_content_types
```

Expected: all PASS.

- [ ] **Step 7: Detect changes before committing**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changes include new web static modules and `build_router`; pre-existing unrelated dirty files may still appear and must not be staged.

- [ ] **Step 8: Commit static modules**

Run:

```bash
git add crates/wukong-web/static/components/chat-thread-header.js \
  crates/wukong-web/static/components/chat-message.js \
  crates/wukong-web/static/components/chat-activity.js \
  crates/wukong-web/static/components/chat-question-card.js \
  crates/wukong-web/src/lib.rs
git commit -m "feat(web): add chat workbench render modules"
```

Expected: commit contains only the new modules and static serving changes.

## Task 2: Refactor WukongChat To Use Workbench Modules

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing `WukongChat`**

Run:

```text
gitnexus_impact({
  target: "WukongChat",
  direction: "upstream",
  file_path: "crates/wukong-web/static/components/wukong-chat.js",
  repo: "Wukong",
  maxDepth: 2
})
```

Expected: LOW blast radius with `static/app.js` importing `WukongChat`. Continue after noting the risk summary.

- [ ] **Step 2: Add failing static string tests for module imports and workbench classes**

In `crates/wukong-web/src/lib.rs`, add this test near the existing `chat_component_*` tests:

```rust
    #[test]
    fn chat_component_uses_workbench_modules() {
        assert!(CHAT_JS.contains("/components/chat-thread-header.js"));
        assert!(CHAT_JS.contains("/components/chat-message.js"));
        assert!(CHAT_JS.contains("/components/chat-activity.js"));
        assert!(CHAT_JS.contains("/components/chat-question-card.js"));
        assert!(CHAT_JS.contains("chat-workbench"));
        assert!(CHAT_JS.contains("conversation-rail"));
    }
```

Also update the existing `chat_component_opens_live_thinking_details()` test so it checks the extracted helper instead of old inline construction strings:

```rust
    #[test]
    fn chat_component_opens_live_thinking_details() {
        assert!(
            CHAT_JS.contains("liveThinkingNode"),
            "chat should use the shared live thinking renderer"
        );
        assert!(
            CHAT_ACTIVITY_JS.contains("details.open = true"),
            "live reasoning details should be open while streaming"
        );
        assert!(
            CHAT_ACTIVITY_JS.contains("className = 'activity-card thinking'"),
            "live thinking should render as an activity card"
        );
    }
```

- [ ] **Step 3: Run the focused test and verify failure**

Run:

```bash
cargo test -p wukong-web chat_component_uses_workbench_modules
```

Expected: FAIL because `wukong-chat.js` does not yet import the new modules or render `chat-workbench` / `conversation-rail`.

- [ ] **Step 4: Import helper modules in `wukong-chat.js`**

At the top of `crates/wukong-web/static/components/wukong-chat.js`, keep the existing imports and add:

```js
import { threadHeaderTemplate } from '/components/chat-thread-header.js';
import { bubbleNode, dateSeparatorNode, messageFrameNode, unreadDividerNode } from '/components/chat-message.js';
import { activityDetailsNode, activityRailNode, liveThinkingNode } from '/components/chat-activity.js';
import { questionCardNode } from '/components/chat-question-card.js';
```

- [ ] **Step 5: Replace the connectedCallback shell markup**

In `connectedCallback()`, replace the current `this.innerHTML = html\`...\`` assignment with:

```js
    this.innerHTML = html`
      <section class="chat-workbench">
        ${unsafe(threadHeaderTemplate())}
        <div class="conversation-rail log" id="log"></div>
        <form id="form" class="composer">
          <div class="textarea-wrapper">
            <textarea id="q" rows="1" autocomplete="off" placeholder="問悟空… (Enter 送出, Shift+Enter 換行)"></textarea>
          </div>
          <button type="submit" class="send-btn" title="送出">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
              <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
            </svg>
          </button>
        </form>
      </section>
    `.toString();
```

This keeps all existing `querySelector('#log')`, `#q`, `#chat-scope`, `#chat-model-status`, `#chat-skill-status`, and `#jump-button` lookups valid.

- [ ] **Step 6: Replace local unread divider helper**

Remove the `unreadDividerNode()` method from `WukongChat`. Existing calls to `this.unreadDividerNode()` inside `renderMessages()` must become:

```js
        unreadDivider = unreadDividerNode();
```

- [ ] **Step 7: Replace `messageNode()` internals with message frame helper**

Change `messageNode(message)` to return the frame from `messageFrameNode()` while preserving code-block enhancement behavior for assistant HTML. The method should look like:

```js
  messageNode(message) {
    const { frame, body } = messageFrameNode(message, {
      attachmentsNode: (msg) => this.attachmentsNode(msg),
    });
    if (message.role === 'assistant') this.enhanceCodeBlocks(body);
    return frame;
  }
```

Then in `renderMessages()`, remove the duplicate attachment append logic if it still exists inside `messageNode()` from the old implementation.

- [ ] **Step 8: Replace date separator construction**

In `renderMessages()`, replace the manual date separator DOM creation:

```js
        const sep = document.createElement('div');
        sep.className = 'date-separator';
        sep.textContent = date;
        nodes.push(sep);
```

with:

```js
        nodes.push(dateSeparatorNode(date));
```

- [ ] **Step 9: Replace `bubble()` and `appendBubble()` internals**

Change `bubble(cls, innerHTML)` to:

```js
  bubble(cls, innerHTML) {
    const div = bubbleNode(cls, innerHTML);
    this.log.appendChild(div);
    void this.scrollToBottomAfterRender();
    return div;
  }
```

Change `appendBubble(cls, innerHTML)` to:

```js
  appendBubble(cls, innerHTML) {
    const div = bubbleNode(cls, innerHTML);
    this.log.appendChild(div);
    return div;
  }
```

- [ ] **Step 10: Replace live thinking node construction**

In `ensureLiveThinking()` and the direct `reasoning` / `tool` event handlers inside `send()`, replace manual `<details class="thinking">` construction with:

```js
      this.liveThinking = liveThinkingNode();
      this.log.appendChild(this.liveThinking);
```

For the local `thinking` variable in `send()`, use:

```js
          thinking = liveThinkingNode();
          this.log.appendChild(thinking);
```

- [ ] **Step 11: Replace lazy activity details construction**

Update `lazyEventsNode(message)` so the first lines are:

```js
    const details = activityDetailsNode({
      className: 'turn-events-group',
      summary: '💭 思考與工具紀錄',
      loadingText: '載入中…',
    });
    const bodySelector = '.activity-card-body';
```

Then replace `details.querySelector('.turn-events-body')` with `details.querySelector(bodySelector)` inside that method.

Update `lazyStepsNode(message)` similarly:

```js
    const details = activityDetailsNode({
      className: 'baton',
      summary: '🔍 悟空·' + (message.step_label || '中間步驟') + ' 的產出',
      loadingText: '載入中…',
    });
    const bodySelector = '.activity-card-body';
```

Use the current label logic from the existing method if it already computes a better summary; do not change the API request URL.

- [ ] **Step 12: Use the question card module**

Replace `renderQuestionCard(request, source)` with a coordinator wrapper:

```js
  renderQuestionCard(request, source) {
    if (this.activeQuestionCard) this.activeQuestionCard.remove();
    const card = questionCardNode(request, source, {
      onSubmit: (req, answers) => this.questionRequest(
        '/api/questions/' + encodeURIComponent(req.request_id) + '/reply',
        { session_id: req.session_id, answers }
      ),
      onReject: (req) => this.questionRequest(
        '/api/questions/' + encodeURIComponent(req.request_id) + '/reject',
        { session_id: req.session_id }
      ),
    });
    if (!card) return null;
    this.activeQuestionCard = card;
    this.log.appendChild(card);
    return card;
  }
```

Keep `questionRequest(path, body)` unchanged.

- [ ] **Step 13: Run focused checks**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-chat.js
node --check crates/wukong-web/static/components/chat-question-card.js
cargo test -p wukong-web chat_component_uses_workbench_modules
cargo test -p wukong-web chat_component_handles_question_events
cargo test -p wukong-web chat_component_opens_live_thinking_details
cargo test -p wukong-web chat_component_preserves_position_when_loading_older
```

Expected: all PASS.

- [ ] **Step 14: Run broader web crate tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 15: Detect changes before committing**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols include `WukongChat` and web static tests. Note unrelated dirty files separately.

- [ ] **Step 16: Commit chat refactor**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/src/lib.rs
git commit -m "refactor(web): render chat as workbench"
```

Expected: commit includes only `wukong-chat.js` and `lib.rs` test additions.

## Task 3: Add Workbench And Activity Rail Styling

**Files:**
- Modify: `crates/wukong-web/static/styles.css`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing style assertions**

Run:

```text
gitnexus_impact({
  target: "chat_styles_include_question_card",
  direction: "upstream",
  file_path: "crates/wukong-web/src/lib.rs",
  repo: "Wukong",
  maxDepth: 2
})
```

Expected: LOW or direct test-only impact.

- [ ] **Step 2: Add failing style assertions**

In `chat_styles_include_question_card()`, add:

```rust
        assert!(STYLES_CSS.contains(".chat-workbench"));
        assert!(STYLES_CSS.contains(".chat-thread-header"));
        assert!(STYLES_CSS.contains(".conversation-rail"));
        assert!(STYLES_CSS.contains(".message-frame"));
        assert!(STYLES_CSS.contains(".activity-rail"));
        assert!(STYLES_CSS.contains("@media (max-width: 720px)"));
```

- [ ] **Step 3: Run the style test and verify failure**

Run:

```bash
cargo test -p wukong-web chat_styles_include_question_card
```

Expected: FAIL because the new workbench classes are not styled yet.

- [ ] **Step 4: Add workbench CSS tokens and layout**

In `crates/wukong-web/static/styles.css`, add these rules near the existing chat styles:

```css
.chat-workbench { display: flex; flex-direction: column; flex: 1; min-height: 0; background: linear-gradient(180deg, #11100e 0%, #181714 100%); color: var(--text-primary); }
.chat-thread-header { align-items: center; background: rgba(38, 40, 39, 0.86); border-bottom: 1px solid rgba(185, 133, 42, 0.24); display: grid; gap: 0.75rem; grid-template-columns: minmax(12rem, 1fr) auto minmax(14rem, auto); padding: 0.7rem 0.9rem; }
.thread-source, .thread-jump, .thread-status { align-items: center; display: flex; flex-wrap: wrap; gap: 0.5rem; }
.thread-status { justify-content: flex-end; }
.conversation-rail { background: radial-gradient(circle at 8% 0%, rgba(185, 133, 42, 0.12), transparent 24rem); }
.message-frame { align-self: stretch; display: grid; gap: 0.35rem; max-width: min(52rem, 92%); }
.message-frame.user { align-self: flex-end; justify-items: end; }
.message-frame.assistant { align-self: flex-start; border-left: 2px solid rgba(185, 133, 42, 0.34); padding-left: 0.75rem; }
.message-meta { color: rgba(236, 227, 207, 0.66); font-size: 0.76rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
.message-content { max-width: 100%; }
.activity-rail { display: grid; gap: 0.45rem; margin: 0.15rem 0 0.25rem; }
.activity-card { background: rgba(38, 40, 39, 0.72); border: 1px solid rgba(185, 133, 42, 0.18); border-radius: 0.8rem; color: var(--text-primary); max-width: min(48rem, 100%); }
.activity-card > summary { cursor: pointer; font-weight: 650; padding: 0.65rem 0.85rem; }
.activity-card-body { border-top: 1px solid rgba(185, 133, 42, 0.14); padding: 0.75rem 0.9rem; }
```

- [ ] **Step 5: Add responsive CSS**

Append:

```css
@media (max-width: 720px) {
  .chat-thread-header { align-items: stretch; grid-template-columns: 1fr; }
  .thread-status { justify-content: flex-start; }
  .message-frame { max-width: 100%; }
  .message-frame.assistant { padding-left: 0.55rem; }
  .activity-card > summary { padding: 0.75rem; }
}
```

- [ ] **Step 6: Run style and syntax checks**

Run:

```bash
cargo test -p wukong-web chat_styles_include_question_card
node --check crates/wukong-web/static/components/wukong-chat.js
```

Expected: PASS.

- [ ] **Step 7: Run full focused web checks**

Run:

```bash
node --test crates/wukong-web/static/lib/unread-marker.test.mjs
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 8: Detect changes before committing**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols include style string tests and static CSS. Note unrelated dirty files separately.

- [ ] **Step 9: Commit styling**

Run:

```bash
git add crates/wukong-web/static/styles.css crates/wukong-web/src/lib.rs
git commit -m "style(web): introduce chat workbench layout"
```

Expected: commit includes only `styles.css` and style test assertions in `lib.rs`.

## Task 4: Final Verification And Manual Review Checklist

**Files:**
- No required code changes.

- [ ] **Step 1: Run final automated checks**

Run:

```bash
node --check crates/wukong-web/static/components/chat-thread-header.js
node --check crates/wukong-web/static/components/chat-message.js
node --check crates/wukong-web/static/components/chat-activity.js
node --check crates/wukong-web/static/components/chat-question-card.js
node --check crates/wukong-web/static/components/wukong-chat.js
node --test crates/wukong-web/static/lib/unread-marker.test.mjs
cargo test -p wukong-web
cargo check --workspace
```

Expected: all PASS.

- [ ] **Step 2: Run GitNexus change detection before completion**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changes map to web static modules, `WukongChat`, static serving, and tests. Pre-existing unrelated dirty files may appear but must not be staged.

- [ ] **Step 3: Manual desktop review**

Run the web console with the project's normal command, for example:

```bash
cargo run -p wukong-web
```

Manual checks in a desktop browser:

- Open `#/chat`. Expected: thread header, conversation rail, and composer render without layout overflow.
- Existing history loads. Expected: user and assistant messages render as message frames.
- Assistant messages with events/steps show activity cards above the answer.
- Expand lazy activity. Expected: content loads or shows retryable inline error without forced scroll to bottom.
- Set an older localStorage unread marker and reload. Expected: unread divider appears and anchors correctly.
- Scroll near top. Expected: older messages prepend without jumping.
- Ask a direct Web Console question. Expected: Enter sends, Shift+Enter inserts newline, reasoning/tool/step/question events still render.

- [ ] **Step 4: Manual mobile review**

Use browser responsive mode around 390px width:

- Open `#/chat`. Expected: thread header stacks into one column.
- Scope selector, jump button, question options, and send button remain tappable.
- Message frames fit width without horizontal scrolling.
- Activity cards are readable and expandable.
- Composer remains visible and usable.

- [ ] **Step 5: Confirm working tree before handoff**

Run:

```bash
git status --short --branch
```

Expected: implementation files are committed. Only pre-existing unrelated dirty files, if any, remain.

## Self-Review Notes

- Spec coverage: the plan covers UX-reference-only boundaries, static module split, thread header, message frames, activity cards, question cards, unread preservation, existing API usage, workbench visual direction, responsive behavior, error handling, testing, and non-goals.
- Placeholder scan: this plan contains no `TBD`, `TODO`, or undefined future work. Each code-changing task includes exact files, snippets, commands, and expected results.
- Type consistency: module exports are `threadHeaderTemplate`, `dateSeparatorNode`, `unreadDividerNode`, `messageFrameNode`, `bubbleNode`, `activityDetailsNode`, `liveThinkingNode`, `activityRailNode`, and `questionCardNode`; imports in `wukong-chat.js` use those exact names.
