# Web Console UI/UX Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the existing Wukong Web Console UI/UX with premium styling, responsive layouts, a multiline auto-resizing composer, interactive code block copy actions, a skeleton loader, and a polished expandable reasoning details view—all adhering to the 100% offline, zero-build, and zero-third-party dependency guidelines.

**Architecture:** Refactor client-side styling inside `styles.css` to use custom CSS variables. Implement multiline textareas, clipboard dynamic button injectors, and loading screens inside `wukong-chat.js`. All styles are native CSS, and features are native Web API calls.

**Tech Stack:** Native HTML5, Vanilla CSS, Vanilla JavaScript (ES Modules, Web Components, EventSource), Rust (axum 0.7 backend server).

## Global Constraints

- **No Third-Party Packages**: Absolutely no external libraries, scripts, fonts (e.g., Google Fonts), or CDN icons are allowed.
- **Offline First**: All assets, layout fallbacks, and rendering behaviors must function completely offline.
- **SafeHTML**: HTML template variables must be escaped or marked with safe explicit custom wrappers (`unsafe`).

---

### Task 1: CSS Variables & Visual Theme Redesign

Establish the core design token system directly in the CSS root and overhaul the app header and sidebar layout for a premium, card-like dark/light theme (with Gold & Red accents).

**Files:**
- Modify: `crates/wukong-web/static/styles.css`

**Interfaces:**
- Consumes: None.
- Produces: CSS design tokens in global root stylesheet.

- [ ] **Step 1: Write CSS variables and global body styles**

In `crates/wukong-web/static/styles.css`, replace lines 1-8 (or equivalent root selector) with:

```css
:root {
  color-scheme: dark;
  --bg-primary: #0f1013;
  --bg-secondary: #17181c;
  --bg-tertiary: #1f2128;
  
  --accent-sun: #ea580c;       /* 觔斗雲橘紅 */
  --accent-gold: #eab308;      /* 火眼金睛金黃 */
  
  --text-primary: #f3f4f6;
  --text-secondary: #9ca3af;
  --text-muted: #6b7280;
  
  --border-color: rgba(255, 255, 255, 0.08);
  --border-focus: rgba(234, 88, 12, 0.4);
  --border-radius: 12px;
  
  --bubble-user: linear-gradient(135deg, #ea580c 0%, #d97706 100%);
  --bubble-assistant: #1f2128;
  
  --transition-smooth: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  --font-modern: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
}

body {
  font-family: var(--font-modern);
  background-color: var(--bg-primary);
  color: var(--text-primary);
  margin: 0;
  display: flex;
  flex-direction: column;
  height: 100vh;
}
```

- [ ] **Step 2: Update Header and navigation style rules**

Replace header and navigation CSS selectors in `crates/wukong-web/static/styles.css`:

```css
header {
  padding: 0.75rem 1.25rem;
  background: rgba(23, 24, 28, 0.8);
  backdrop-filter: blur(12px) saturate(180%);
  -webkit-backdrop-filter: blur(12px) saturate(180%);
  border-bottom: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.15);
  z-index: 10;
}
header h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 700;
  letter-spacing: -0.025em;
  background: linear-gradient(135deg, var(--accent-gold) 0%, var(--accent-sun) 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
}
nav {
  display: flex;
  gap: 1rem;
}
nav a {
  color: var(--text-secondary);
  text-decoration: none;
  font-size: 0.95rem;
  font-weight: 500;
  transition: var(--transition-smooth);
  opacity: 0.8;
}
nav a:hover, nav a.active {
  color: var(--accent-gold);
  opacity: 1;
}
```

- [ ] **Step 3: Redesign User and Assistant chat bubble styles**

Replace bubble rules in `crates/wukong-web/static/styles.css` (lines 24-30 or equivalent):

```css
.bubble {
  max-width: 80%;
  padding: 0.75rem 1rem;
  border-radius: var(--border-radius);
  line-height: 1.5;
  font-size: 0.975rem;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  transition: var(--transition-smooth);
}
.bubble.user {
  align-self: flex-end;
  background: var(--bubble-user);
  color: #fff;
  border-bottom-right-radius: 4px;
}
.bubble.assistant {
  align-self: flex-start;
  background: var(--bubble-assistant);
  color: var(--text-primary);
  border-bottom-left-radius: 4px;
  border: 1px solid var(--border-color);
}
.bubble.status {
  align-self: flex-start;
  background: transparent;
  color: var(--accent-gold);
  border: none;
  box-shadow: none;
  opacity: 0.9;
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
```

- [ ] **Step 4: Commit style updates**

```bash
git add crates/wukong-web/static/styles.css
git commit -m "style(web): implement premium CSS theme variables and header redesign"
```

---

### Task 2: Multiline auto-resizing Composer

Replace the single-line input box with a textarea that expands as user types, supporting `Enter` to submit and `Shift + Enter` to line-break.

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/static/styles.css`

**Interfaces:**
- Consumes: CSS variables `--bg-secondary`, `--accent-sun`, `--transition-smooth`.
- Produces: Auto-resizing multiline text input in the composer form.

- [ ] **Step 1: Replace input element with textarea and Inline SVG in JS template**

In `crates/wukong-web/static/components/wukong-chat.js`, replace lines 16-19 (the composer form inside `innerHTML` template):

```javascript
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
```

- [ ] **Step 2: Initialize query selector and bind resizing listeners**

In `crates/wukong-web/static/components/wukong-chat.js`, replace the initialization (lines 22-23) and register input events:

```javascript
    this.log = this.querySelector('#log');
    const textarea = this.querySelector('#q');
    this.input = textarea; // Preserve compatibility with existing send() code referencing this.input
    this.scopeSelect = this.querySelector('#chat-scope');
    
    // Auto-resizing logic
    textarea.addEventListener('input', function () {
      this.style.height = 'auto';
      this.style.height = Math.min(this.scrollHeight, 200) + 'px';
    });
    
    // Keyboard handlers
    textarea.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        this.send();
        textarea.style.height = 'auto';
      }
    });
```

- [ ] **Step 3: Add Composer styling rules**

Append to `crates/wukong-web/static/styles.css`:

```css
.composer {
  display: flex;
  align-items: flex-end;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}
.textarea-wrapper {
  flex: 1;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius);
  padding: 0.5rem 0.75rem;
  transition: var(--transition-smooth);
}
.textarea-wrapper:focus-within {
  border-color: var(--accent-sun);
  box-shadow: 0 0 0 2px var(--border-focus);
}
.composer textarea {
  width: 100%;
  border: none;
  background: transparent;
  color: inherit;
  font-family: inherit;
  font-size: 0.975rem;
  resize: none;
  outline: none;
  line-height: 1.4;
  max-height: 200px;
  box-sizing: border-box;
  padding: 0;
  margin: 0;
}
.send-btn {
  background: var(--accent-sun);
  color: #fff;
  border: none;
  border-radius: 50%;
  width: 2.25rem;
  height: 2.25rem;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: var(--transition-smooth);
  padding: 0;
  flex-shrink: 0;
  margin-bottom: 0.15rem;
}
.send-btn:hover {
  background: #ea580cde;
  transform: scale(1.05);
}
.send-btn:active {
  transform: scale(0.95);
}
```

- [ ] **Step 4: Commit Composer updates**

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css
git commit -m "feat(web): replace input with dynamic auto-resizing textarea composer"
```

---

### Task 3: Expandable reasoning logs (美化思考過程)

Refactor `<details>` to feel like a premium expandable logs widget with Gold Accent and rotate transition indicator.

**Files:**
- Modify: `crates/wukong-web/static/styles.css`

**Interfaces:**
- Consumes: CSS variables `--accent-gold`, `--text-secondary`.
- Produces: Gold themed details summary layouts.

- [ ] **Step 1: Replace old details/thinking styles**

In `crates/wukong-web/static/styles.css`, replace lines 34-36 (or the existing `.thinking` styles):

```css
.thinking {
  align-self: flex-start;
  max-width: 80%;
  width: 100%;
  margin: 0.5rem 0;
  border: 1px solid rgba(234, 179, 8, 0.15);
  border-radius: 8px;
  background: rgba(234, 179, 8, 0.03);
  overflow: hidden;
  transition: var(--transition-smooth);
}
.thinking summary {
  padding: 0.5rem 0.75rem;
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--accent-gold);
  cursor: pointer;
  list-style: none;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  user-select: none;
  background: rgba(234, 179, 8, 0.05);
  transition: var(--transition-smooth);
}
.thinking summary::-webkit-details-marker {
  display: none;
}
.thinking summary::before {
  content: '▶';
  font-size: 0.7rem;
  display: inline-block;
  transition: transform 0.2s ease;
}
.thinking[open] summary::before {
  transform: rotate(90deg);
}
.thinking[open] summary {
  border-bottom: 1px solid rgba(234, 179, 8, 0.12);
}
.thinking .reasoning {
  padding: 0.75rem;
  font-family: monospace;
  font-size: 0.85rem;
  white-space: pre-wrap;
  color: var(--text-secondary);
  line-height: 1.4;
  margin: 0;
  max-height: 15rem;
  overflow-y: auto;
}
```

- [ ] **Step 2: Commit thinking block updates**

```bash
git add crates/wukong-web/static/styles.css
git commit -m "style(web): style reasoning details block with custom gold indicators"
```

---

### Task 4: Inline copy tools for Code Blocks

Add copy action button dynamically on hover for each parsed code block in markdown outputs.

**Files:**
- Modify: `crates/wukong-web/static/styles.css`
- Modify: `crates/wukong-web/static/components/wukong-chat.js`

**Interfaces:**
- Consumes: Rendered `<pre>` containers in assistant responses.
- Produces: `enhanceCodeBlocks` DOM utility dynamically injecting inline buttons.

- [ ] **Step 1: Style Code Blocks and Copy Button**

Replace lines 31-33 in `crates/wukong-web/static/styles.css` (or equivalent `.bubble pre` selectors):

```css
.bubble pre {
  position: relative;
  background: #0002;
  padding: 0.75rem;
  border-radius: var(--border-radius);
  border: 1px solid rgba(255, 255, 255, 0.05);
  overflow-x: auto;
}
.bubble pre code {
  font-family: SFMono-Regular, Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 0.875rem;
}
.copy-code-btn {
  position: absolute;
  top: 0.5rem;
  right: 0.5rem;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  padding: 0.25rem 0.5rem;
  font-size: 0.75rem;
  cursor: pointer;
  opacity: 0;
  transition: var(--transition-smooth);
}
.bubble pre:hover .copy-code-btn {
  opacity: 1;
}
.copy-code-btn:hover {
  background: var(--accent-sun);
  color: #fff;
}
```

- [ ] **Step 2: Implement dynamic copy logic**

Add the `enhanceCodeBlocks` method right above `send()` in `crates/wukong-web/static/components/wukong-chat.js`:

```javascript
  enhanceCodeBlocks(container) {
    const pres = container.querySelectorAll('pre');
    pres.forEach((pre) => {
      if (pre.querySelector('.copy-code-btn')) return;
      pre.style.position = 'relative';
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'copy-code-btn';
      button.textContent = '複製';
      
      button.addEventListener('click', async () => {
        const codeText = pre.querySelector('code')?.textContent || pre.textContent;
        try {
          await navigator.clipboard.writeText(codeText);
          button.textContent = '已複製！';
          setTimeout(() => { button.textContent = '複製'; }, 2000);
        } catch (_err) {
          button.textContent = '複製失敗';
        }
      });
      pre.appendChild(button);
    });
  }
```

- [ ] **Step 3: Connect code block enhancement to rendering cycles**

In `crates/wukong-web/static/components/wukong-chat.js`, invoke this method inside `renderMessages` and `send()` EventSource listeners.

In `renderMessages(messages, mode)`:
Replace line 125-126 inside the loop (around where bubble is generated):
```javascript
      const bubbleNode = this.messageNode(message);
      if (message.role === 'assistant') {
        this.enhanceCodeBlocks(bubbleNode);
      }
      nodes.push(bubbleNode);
```

In `send()`:
Replace lines 221-225 (the `answer` listener):
```javascript
    es.addEventListener('answer', (ev) => {
      progress.remove();
      // Server already produced safe HTML; mark it trusted.
      const div = this.bubble('assistant', unsafe(ev.data).toString());
      this.enhanceCodeBlocks(div);
    });
```

- [ ] **Step 4: Commit Code Copy updates**

```bash
git add crates/wukong-web/static/styles.css crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat(web): implement copy-to-clipboard button injectors for markdown code blocks"
```

---

### Task 5: Skeleton Screen Loader & Smooth Scrolling

Provide visual skeleton placeholders during `loadOlder` calls to prevent layout jumps and ensure smooth scroll anchors.

**Files:**
- Modify: `crates/wukong-web/static/styles.css`
- Modify: `crates/wukong-web/static/components/wukong-chat.js`

**Interfaces:**
- Consumes: CSS layout styles.
- Produces: Skeleton DOM nodes prepend during infinite scroll async triggers.

- [ ] **Step 1: Write Skeleton Animation in CSS**

Append to `crates/wukong-web/static/styles.css`:

```css
@keyframes skeleton-pulse {
  0% { opacity: 0.5; }
  50% { opacity: 1; }
  100% { opacity: 0.5; }
}
.skeleton-loader {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 1rem;
  width: 80%;
  animation: skeleton-pulse 1.5s ease-in-out infinite;
}
.skeleton-bar {
  background: var(--bg-tertiary);
  border-radius: 4px;
  height: 0.8rem;
}
.skeleton-bar.w-full { width: 100%; }
.skeleton-bar.w-75 { width: 75%; }
.skeleton-bar.w-50 { width: 50%; }

.log {
  scroll-behavior: smooth;
}
```

- [ ] **Step 2: Append skeleton elements on fetch start**

In `crates/wukong-web/static/components/wukong-chat.js`, rewrite `loadOlder()`:

```javascript
  async loadOlder() {
    if (this.loadingOlder || !this.hasMore || !this.oldestId) return;
    this.loadingOlder = true;

    const skeleton = document.createElement('div');
    skeleton.className = 'skeleton-loader';
    skeleton.innerHTML = `
      <div class="skeleton-bar w-full"></div>
      <div class="skeleton-bar w-75"></div>
      <div class="skeleton-bar w-50"></div>
    `;
    this.log.prepend(skeleton);

    try {
      const data = await this.fetchMessages('before=' + encodeURIComponent(this.oldestId) + '&limit=10');
      this.hasMore = data.has_more;
      this.renderMessages(data.messages, 'prepend');
    } catch (_err) {
      const note = document.createElement('p');
      note.className = 'load-error';
      note.textContent = '載入較舊訊息失敗，請重試。';
      this.log.prepend(note);
    } finally {
      skeleton.remove();
      this.loadingOlder = false;
    }
  }
```

- [ ] **Step 3: Commit Skeleton Loader updates**

```bash
git add crates/wukong-web/static/styles.css crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat(web): add CSS-pulse skeleton loader for paginated history fetching"
```

---

### Task 6: Compile & Test Verification

Compile the workspace and execute all workspace unit tests to verify no compilation regressions occur.

**Files:**
- Test: Build and test suite verification.

**Interfaces:**
- Consumes: None.
- Produces: Clean Cargo build and all passing tests.

- [ ] **Step 1: Compile the wukong-web crate**

Run:
```bash
cargo build -p wukong-web
```
Expected: OK.

- [ ] **Step 2: Run all workspace tests**

Run:
```bash
cargo test
```
Expected: PASS — all 242+ tests pass.

- [ ] **Step 3: Check warnings using Clippy**

Run:
```bash
cargo clippy --all-targets -- -D warnings
```
Expected: No compiler warnings or clippy violations.
