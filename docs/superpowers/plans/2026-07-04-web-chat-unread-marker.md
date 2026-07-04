# Web Chat Unread Marker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-scope Web Console unread markers that restore the user's last seen point and clear after real interaction.

**Architecture:** Keep read state local to the browser using `localStorage`, isolated behind a small pure JS helper module. `wukong-chat.js` owns DOM insertion, initial anchoring, interaction clearing, and scope switching. No backend chat-history schema or `/api/chat/messages` response shape changes are required.

**Tech Stack:** Plain custom elements, browser `localStorage`, ES modules, Node built-in `node:test` for pure helper tests, Rust `include_str!` static asset serving through `wukong-web`.

---

## File Structure

- Create: `crates/wukong-web/static/lib/unread-marker.mjs`
  - Pure helper functions for marker keying, safe storage reads/writes, numeric id parsing, latest id detection, and unread divider insertion index calculation.
- Create: `crates/wukong-web/static/lib/unread-marker.test.mjs`
  - Node tests for the pure helper functions. This avoids introducing a browser test runner.
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
  - Import helper functions, add unread state fields, insert/clear divider DOM, update localStorage marker, and adjust first-load anchoring.
- Modify: `crates/wukong-web/static/styles.css`
  - Add `.unread-divider` styling and disable scroll anchoring inside `.log`.
- Modify: `crates/wukong-web/src/lib.rs`
  - Serve `/lib/unread-marker.mjs` as a static asset and extend static asset content-type tests.

## Task 1: Pure Unread Marker Helper

**Files:**
- Create: `crates/wukong-web/static/lib/unread-marker.mjs`
- Create: `crates/wukong-web/static/lib/unread-marker.test.mjs`

- [ ] **Step 1: Write the failing helper tests**

Create `crates/wukong-web/static/lib/unread-marker.test.mjs`:

```js
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  firstUnreadIndex,
  lastSeenKey,
  latestMessageId,
  readLastSeenMessageId,
  writeLastSeenMessageId,
} from './unread-marker.mjs';

class MemoryStorage {
  constructor(entries = {}) {
    this.map = new Map(Object.entries(entries));
  }

  getItem(key) {
    return this.map.has(key) ? this.map.get(key) : null;
  }

  setItem(key, value) {
    this.map.set(key, String(value));
  }
}

test('lastSeenKey is scoped', () => {
  assert.equal(lastSeenKey('global'), 'wukong.chat.lastSeenMessageId:global');
  assert.equal(lastSeenKey('user:tg-915354960'), 'wukong.chat.lastSeenMessageId:user:tg-915354960');
});

test('readLastSeenMessageId ignores missing invalid and non-positive values', () => {
  assert.equal(readLastSeenMessageId(new MemoryStorage(), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: 'abc' }), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: '0' }), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: '-7' }), 'global'), null);
});

test('readLastSeenMessageId reads positive integer values', () => {
  const storage = new MemoryStorage({ [lastSeenKey('global')]: '42' });
  assert.equal(readLastSeenMessageId(storage, 'global'), 42);
});

test('writeLastSeenMessageId writes only positive latest ids', () => {
  const storage = new MemoryStorage();
  assert.equal(writeLastSeenMessageId(storage, 'global', null), false);
  assert.equal(writeLastSeenMessageId(storage, 'global', 0), false);
  assert.equal(writeLastSeenMessageId(storage, 'global', 7), true);
  assert.equal(storage.getItem(lastSeenKey('global')), '7');
});

test('latestMessageId returns largest numeric message id', () => {
  assert.equal(latestMessageId([{ id: 3 }, { id: '9' }, { id: 'bad' }, { id: 5 }]), 9);
  assert.equal(latestMessageId([{ id: 'bad' }]), null);
  assert.equal(latestMessageId([]), null);
});

test('firstUnreadIndex returns -1 when no stored marker exists', () => {
  assert.equal(firstUnreadIndex([{ id: 1 }, { id: 2 }], null), -1);
});

test('firstUnreadIndex finds first message newer than marker', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }, { id: 12 }], 10), 1);
});

test('firstUnreadIndex returns first message when whole latest page is new', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 3), 0);
});

test('firstUnreadIndex returns -1 when marker is current or newer', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 11), -1);
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 99), -1);
});
```

- [ ] **Step 2: Run helper tests to verify they fail**

Run: `node --test crates/wukong-web/static/lib/unread-marker.test.mjs`

Expected: FAIL with module-not-found or missing export error for `./unread-marker.mjs`.

- [ ] **Step 3: Implement the minimal helper module**

Create `crates/wukong-web/static/lib/unread-marker.mjs`:

```js
const KEY_PREFIX = 'wukong.chat.lastSeenMessageId:';

export function lastSeenKey(scope) {
  return KEY_PREFIX + String(scope || 'global');
}

function parsePositiveInteger(value) {
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

export function readLastSeenMessageId(storage, scope) {
  try {
    return parsePositiveInteger(storage.getItem(lastSeenKey(scope)));
  } catch (_err) {
    return null;
  }
}

export function writeLastSeenMessageId(storage, scope, messageId) {
  const parsed = parsePositiveInteger(messageId);
  if (parsed === null) return false;
  try {
    storage.setItem(lastSeenKey(scope), String(parsed));
    return true;
  } catch (_err) {
    return false;
  }
}

export function latestMessageId(messages) {
  let latest = null;
  for (const message of messages || []) {
    const id = parsePositiveInteger(message && message.id);
    if (id !== null && (latest === null || id > latest)) latest = id;
  }
  return latest;
}

export function firstUnreadIndex(messages, marker) {
  const parsedMarker = parsePositiveInteger(marker);
  if (parsedMarker === null) return -1;
  const list = Array.isArray(messages) ? messages : [];
  for (let index = 0; index < list.length; index += 1) {
    const id = parsePositiveInteger(list[index] && list[index].id);
    if (id !== null && id > parsedMarker) return index;
  }
  return -1;
}
```

- [ ] **Step 4: Run helper tests to verify they pass**

Run: `node --test crates/wukong-web/static/lib/unread-marker.test.mjs`

Expected: PASS with 9 tests passing.

- [ ] **Step 5: Commit helper module**

Run:

```bash
git add crates/wukong-web/static/lib/unread-marker.mjs crates/wukong-web/static/lib/unread-marker.test.mjs
git commit -m "feat(web): add unread marker helpers"
```

Expected: commit includes only the helper and its test.

## Task 2: Serve Helper Static Asset

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write the failing static asset assertion**

Find the existing `serves_static_assets_with_content_types` test in `crates/wukong-web/src/lib.rs`. Add this path to the list of static assets it checks:

```rust
"/lib/unread-marker.mjs",
```

- [ ] **Step 2: Run the static asset test to verify it fails**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: FAIL because `/lib/unread-marker.mjs` is not routed yet and returns `404 Not Found`.

- [ ] **Step 3: Add the static asset include and handler**

In `crates/wukong-web/src/lib.rs`, add the static asset constant near the other `include_str!` constants:

```rust
const UNREAD_MARKER_JS: &str = include_str!("../static/lib/unread-marker.mjs");
```

Add the handler near the other static asset handler functions:

```rust
async fn unread_marker_js() -> axum::response::Response {
    asset("text/javascript; charset=utf-8", UNREAD_MARKER_JS)
}
```

Add the route in `build_router()` next to `/lib/html.js`:

```rust
.route("/lib/unread-marker.mjs", axum::routing::get(unread_marker_js))
```

- [ ] **Step 4: Run the static asset test to verify it passes**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: PASS.

- [ ] **Step 5: Commit static asset serving**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat(web): serve unread marker helper"
```

Expected: commit includes only `crates/wukong-web/src/lib.rs`.

## Task 3: Integrate Unread Marker Into Chat Component

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`

- [ ] **Step 1: Run impact analysis before editing the component**

Run this GitNexus MCP impact query before editing `WukongChat`:

```text
gitnexus_impact({
  target: "WukongChat",
  direction: "upstream",
  file_path: "crates/wukong-web/static/components/wukong-chat.js",
  repo: "Wukong",
  maxDepth: 2
})
```

Expected: document the risk summary in the implementation notes before editing. If GitNexus reports HIGH or CRITICAL risk, continue because this plan specifically targets the chat component and the verification steps below cover the affected behavior.

- [ ] **Step 2: Import helper functions**

At the top of `crates/wukong-web/static/components/wukong-chat.js`, add:

```js
import {
  firstUnreadIndex,
  latestMessageId,
  readLastSeenMessageId,
  writeLastSeenMessageId,
} from '/lib/unread-marker.mjs';
```

- [ ] **Step 3: Add unread state fields**

In `connectedCallback()`, after `this.activeQuestionCard = null;`, add:

```js
this.unreadDivider = null;
this.userInteractedWithChat = false;
this.initialAnchoring = false;
```

- [ ] **Step 4: Add interaction listeners**

In `connectedCallback()`, after the existing `this.log.addEventListener('scroll', ...)` block, add:

```js
const markInteraction = () => this.handleChatInteraction();
this.log.addEventListener('wheel', markInteraction, { passive: true });
this.log.addEventListener('touchstart', markInteraction, { passive: true });
this.log.addEventListener('pointerdown', markInteraction);
this.log.addEventListener('keydown', markInteraction);
```

Update the existing `scroll` handler so programmatic initial anchoring does not trigger older-message loading:

```js
this.log.addEventListener('scroll', () => {
  if (this.initialAnchoring) return;
  if (this.log.scrollTop < 80) this.loadOlder();
});
```

- [ ] **Step 5: Add unread helper methods to `WukongChat`**

Insert these methods after `resetMessages()`:

```js
storage() {
  return window.localStorage;
}

currentLatestMessageId() {
  const ids = Array.from(this.log.querySelectorAll('[data-message-id]'))
    .map((node) => Number.parseInt(node.dataset.messageId, 10))
    .filter((id) => Number.isFinite(id) && id > 0);
  return ids.length ? Math.max(...ids) : null;
}

recordLatestSeenMessage() {
  const latest = this.currentLatestMessageId();
  if (latest !== null) writeLastSeenMessageId(this.storage(), this.selectedScope, latest);
}

removeUnreadDivider({ record = true } = {}) {
  if (this.unreadDivider) {
    this.unreadDivider.remove();
    this.unreadDivider = null;
  }
  if (record) this.recordLatestSeenMessage();
}

handleChatInteraction() {
  if (this.initialAnchoring) return;
  this.userInteractedWithChat = true;
  if (this.unreadDivider) this.removeUnreadDivider({ record: true });
}

unreadDividerNode() {
  const div = document.createElement('div');
  div.className = 'unread-divider';
  div.textContent = '以下是上次離開後的新紀錄';
  return div;
}
```

- [ ] **Step 6: Reset unread state on message reset**

Update `resetMessages()` so it clears unread state before clearing the log:

```js
resetMessages() {
  this.hasMore = false;
  this.oldestId = null;
  this.renderedMessageIds.clear();
  this.liveProgress = null;
  this.liveThinking = null;
  this.activeQuestionCard = null;
  this.unreadDivider = null;
  this.userInteractedWithChat = false;
  this.initialAnchoring = false;
  this.log.innerHTML = '';
}
```

- [ ] **Step 7: Make render return anchor metadata**

Change `renderMessages(messages, mode)` to accept an options object and return the unread divider when one is inserted:

```js
renderMessages(messages, mode, options = {}) {
  if (mode !== 'prepend') this.renderedMessageIds.clear();
  const unreadIndex = options.unreadIndex ?? -1;
  const nodes = [];
  let unreadDivider = null;
  let lastDate = null;
  for (const [index, message] of messages.entries()) {
    if (index === unreadIndex) {
      unreadDivider = this.unreadDividerNode();
      nodes.push(unreadDivider);
    }
    const date = new Date(message.created_at * 1000).toLocaleDateString('zh-TW', {
      year: 'numeric', month: 'long', day: 'numeric',
    });
    if (date !== lastDate) {
      const sep = document.createElement('div');
      sep.className = 'date-separator';
      sep.textContent = date;
      nodes.push(sep);
      lastDate = date;
    }
    const bubbleNode = this.messageNode(message);
    this.renderedMessageIds.add(String(message.id));
    if (message.role === 'assistant') {
      if (message.event_count > 0) nodes.push(this.lazyEventsNode(message));
      if (message.step_count > 0) nodes.push(this.lazyStepsNode(message));
      this.enhanceCodeBlocks(bubbleNode);
    }
    nodes.push(bubbleNode);
  }
  if (mode === 'prepend') {
    const previousHeight = this.log.scrollHeight;
    for (const node of nodes.reverse()) this.log.prepend(node);
    this.preserveScrollPosition(previousHeight);
  } else {
    this.log.innerHTML = '';
    for (const node of nodes) this.log.appendChild(node);
    this.unreadDivider = unreadDivider;
    if (!unreadDivider) void this.scrollToBottomAfterRender();
  }
  this.oldestId = this.log.querySelector('[data-message-id]')?.dataset.messageId || null;
  return { unreadDivider };
}
```

- [ ] **Step 8: Add initial anchor method**

Insert after `scrollToBottomAfterRender()`:

```js
async anchorInitialView(unreadDivider) {
  this.initialAnchoring = true;
  await this.waitForLayoutContent();
  if (unreadDivider) {
    unreadDivider.scrollIntoView({ block: 'center', behavior: 'auto' });
  } else {
    this.scrollToBottom();
  }
  await this.nextFrame();
  this.initialAnchoring = false;
}
```

- [ ] **Step 9: Update `loadLatest()` to apply marker logic**

Replace the successful branch of `loadLatest()` with:

```js
const marker = readLastSeenMessageId(this.storage(), this.selectedScope);
const unreadIndex = firstUnreadIndex(data.messages, marker);
const { unreadDivider } = this.renderMessages(data.messages, 'replace', { unreadIndex });
await this.anchorInitialView(unreadDivider);
if (marker === null) {
  const latest = latestMessageId(data.messages);
  if (latest !== null) writeLastSeenMessageId(this.storage(), this.selectedScope, latest);
}
```

The full method should remain:

```js
async loadLatest() {
  try {
    const data = await this.fetchMessages('limit=10');
    if (!data.messages.length) {
      this.log.innerHTML = '<p class="empty-state">還沒有對話，問悟空第一個問題。</p>';
      return;
    }
    this.hasMore = data.has_more;
    const marker = readLastSeenMessageId(this.storage(), this.selectedScope);
    const unreadIndex = firstUnreadIndex(data.messages, marker);
    const { unreadDivider } = this.renderMessages(data.messages, 'replace', { unreadIndex });
    await this.anchorInitialView(unreadDivider);
    if (marker === null) {
      const latest = latestMessageId(data.messages);
      if (latest !== null) writeLastSeenMessageId(this.storage(), this.selectedScope, latest);
    }
  } catch (err) {
    this.log.innerHTML = '<p class="empty-state">無法讀取對話歷史：' + escapeHTML(err.message) + '</p>';
  }
}
```

- [ ] **Step 10: Update scope switching**

Replace the scope select change listener in `connectedCallback()` with:

```js
this.scopeSelect.addEventListener('change', () => {
  if (!this.unreadDivider || this.userInteractedWithChat) this.recordLatestSeenMessage();
  this.closeLiveStream();
  this.selectedScope = this.scopeSelect.value;
  this.liveCursor = 0;
  this.resetMessages();
  this.loadLatest().then(() => this.startLiveStream());
});
```

- [ ] **Step 11: Update send behavior**

At the start of `send()`, after `if (!text) return;`, add:

```js
this.handleChatInteraction();
```

This clears the divider and records the latest loaded id before the user appends a new message.

- [ ] **Step 12: Run browser helper tests and web crate tests**

Run:

```bash
node --test crates/wukong-web/static/lib/unread-marker.test.mjs
cargo test -p wukong-web serves_static_assets_with_content_types
```

Expected: both commands PASS.

- [ ] **Step 13: Commit chat integration**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat(web): restore chat unread position"
```

Expected: commit includes only `wukong-chat.js`.

## Task 4: Styling And Final Verification

**Files:**
- Modify: `crates/wukong-web/static/styles.css`

- [ ] **Step 1: Add unread divider CSS**

In `crates/wukong-web/static/styles.css`, update `.log` to disable browser scroll anchoring and add divider styling after `.date-separator` if present, or after `.log` if not:

```css
.log {
  flex: 1;
  overflow-y: auto;
  padding: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  scroll-behavior: smooth;
  overflow-anchor: none;
}

.unread-divider {
  align-self: stretch;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  color: var(--accent-gold);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  opacity: 0.92;
}

.unread-divider::before,
.unread-divider::after {
  content: "";
  flex: 1;
  height: 1px;
  background: linear-gradient(90deg, transparent, rgba(234, 179, 8, 0.55), transparent);
}
```

- [ ] **Step 2: Run formatting-neutral checks**

Run:

```bash
node --test crates/wukong-web/static/lib/unread-marker.test.mjs
cargo test -p wukong-web serves_static_assets_with_content_types
cargo check --workspace
```

Expected: all commands PASS.

- [ ] **Step 3: Manual verification in browser**

Run the Web Console using the project's normal command, for example:

```bash
cargo run -p wukong-web
```

Manual checks:

- Open `#/chat` with no `localStorage` key for the selected scope. Expected: no unread divider, viewport opens at bottom, localStorage receives latest loaded message id.
- Set localStorage manually to an older id for the selected scope. Expected: reload inserts `以下是上次離開後的新紀錄` before the first newer loaded message and anchors there.
- Scroll or click inside the chat log. Expected: divider disappears and localStorage updates to the largest loaded message id.
- Reload. Expected: those same messages are not marked unread again.
- Scroll near the top to trigger `loadOlder()`. Expected: older messages prepend without moving the divider or changing the marker unexpectedly.
- Switch scopes. Expected: each scope uses an independent localStorage key.

- [ ] **Step 4: Run GitNexus change detection before final commit**

Run GitNexus change detection for the current repo:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols map to `wukong-web` static assets and static serving only. Note any reported pre-existing dirty files separately and do not stage unrelated changes.

- [ ] **Step 5: Commit styling and verification updates**

Run:

```bash
git add crates/wukong-web/static/styles.css
git commit -m "style(web): mark unread chat records"
```

Expected: commit includes only `styles.css`.

## Final Verification

- [ ] Run full focused checks:

```bash
node --test crates/wukong-web/static/lib/unread-marker.test.mjs
cargo test -p wukong-web
cargo check --workspace
```

Expected: all commands PASS.

- [ ] Confirm git status contains no unintended staged files:

```bash
git status --short
```

Expected: only pre-existing unrelated files may remain, such as `AGENTS.md` or `CLAUDE.md`; the unread marker implementation files should be committed.

## Self-Review Notes

- Spec coverage: localStorage per scope, both roles counted as unread, divider insertion, first-load anchor, interaction clearing, scope independence, no backend schema change, error handling, and pagination preservation are each covered by tasks above.
- Placeholder scan: this plan contains no `TBD`, `TODO`, or undefined future work. Live stream read-marker updates are intentionally not implemented because the spec scopes live stream behavior to preserving existing sticky-bottom behavior.
- Type consistency: helper names are `lastSeenKey`, `readLastSeenMessageId`, `writeLastSeenMessageId`, `latestMessageId`, and `firstUnreadIndex`; the chat integration imports exactly those names.
