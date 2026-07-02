import { html, unsafe, escapeHTML } from '/lib/html.js';

// <wukong-chat>: message log + composer + SSE wiring. Self-contained custom
// element (no router/services), per plainvanillaweb core conventions.
export class WukongChat extends HTMLElement {
  connectedCallback() {
    this.scopes = [];
    this.selectedScope = '';
    this.liveStream = null;
    this.liveCursor = 0;
    this.renderedMessageIds = new Set();
    this.liveProgress = null;
    this.liveThinking = null;
    this.innerHTML = html`
      <div class="chat-toolbar">
        <label class="chat-source">來源 <select id="chat-scope"></select></label>
        <label>跳到日期 <input id="jump-date" type="date" /></label>
        <button id="jump-button" type="button">前往</button>
        <span id="chat-model-status" class="tag">模型：載入中</span>
        <span id="chat-skill-status" class="tag">技能偏好：Phase 2</span>
      </div>
      <div class="log" id="log"></div>
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
    `.toString();
    this.log = this.querySelector('#log');
    const textarea = this.querySelector('#q');
    this.input = textarea;
    this.scopeSelect = this.querySelector('#chat-scope');
    this.modelStatus = this.querySelector('#chat-model-status');
    this.skillStatus = this.querySelector('#chat-skill-status');
    
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
    this.loadingOlder = false;
    this.hasMore = false;
    this.oldestId = null;
    this.scopeSelect.addEventListener('change', () => {
      this.closeLiveStream();
      this.selectedScope = this.scopeSelect.value;
      this.liveCursor = 0;
      this.resetMessages();
      this.loadLatest().then(() => this.startLiveStream());
    });
    this.querySelector('#form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.send();
    });
    this.querySelector('#jump-button').addEventListener('click', () => this.jumpToDate());
    this.log.addEventListener('scroll', () => {
      if (this.log.scrollTop < 80) this.loadOlder();
    });
    this.initialize();
  }

  disconnectedCallback() {
    this.closeLiveStream();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  scopeParam(prefix = '&') {
    return this.selectedScope ? prefix + 'scope=' + encodeURIComponent(this.selectedScope) : '';
  }

  chatUrl(path, params = {}) {
    const search = new URLSearchParams();
    if (window.WUKONG_TOKEN) search.set('token', window.WUKONG_TOKEN);
    if (this.selectedScope) search.set('scope', this.selectedScope);
    Object.entries(params).forEach(([key, value]) => {
      if (value !== undefined && value !== null && value !== '') search.set(key, value);
    });
    const qs = search.toString();
    return qs ? path + '?' + qs : path;
  }

  async initialize() {
    await Promise.all([this.loadModelStatus(), this.loadSkillStatus()]);
    await this.loadScopes();
    await this.loadLatest();
    this.startLiveStream();
  }

  isTelegramScope() {
    return this.selectedScope && this.selectedScope.startsWith('user:tg-');
  }

  closeLiveStream() {
    if (this.liveStream) {
      this.liveStream.close();
      this.liveStream = null;
    }
    this.liveProgress = null;
    this.liveThinking = null;
  }

  startLiveStream() {
    this.closeLiveStream();
    if (!this.isTelegramScope()) return;
    const stream = new EventSource(this.chatUrl('/api/chat/stream', { after: this.liveCursor }));
    this.liveStream = stream;
    stream.addEventListener('user', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('role', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('reasoning', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('tool', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('step', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('answer', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('error', (ev) => {
      if (ev.data) this.handleLiveEvent(ev);
    });
  }

  async loadModelStatus() {
    if (!this.modelStatus) return;
    const token = window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
    const resp = await fetch('/api/settings/model' + token);
    if (!resp.ok) {
      this.modelStatus.textContent = '模型：未知';
      return;
    }
    const data = await resp.json();
    this.modelStatus.textContent = data.model ? '模型：' + data.model : '模型：agent 預設';
  }

  async loadSkillStatus() {
    if (!this.skillStatus) return;
    const token = window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
    const resp = await fetch('/api/skills/preferences' + token);
    if (!resp.ok) {
      this.skillStatus.textContent = '技能偏好：未知';
      return;
    }
    const data = await resp.json();
    if (!data.enabled) {
      this.skillStatus.textContent = '技能偏好：未啟用';
      return;
    }
    const picks = [...(data.roles || []), ...(data.skills || [])];
    this.skillStatus.textContent = picks.length
      ? '技能偏好：' + picks.join(' + ')
      : '技能偏好：已啟用';
  }

  resetMessages() {
    this.hasMore = false;
    this.oldestId = null;
    this.renderedMessageIds.clear();
    this.liveProgress = null;
    this.liveThinking = null;
    this.log.innerHTML = '';
  }

  async loadScopes() {
    try {
      const resp = await fetch('/api/chat/scopes' + this.tokenParam('?'));
      if (!resp.ok) return;
      this.scopes = await resp.json();
      if (!this.selectedScope && this.scopes.length > 0) {
        const telegram = this.scopes.find((s) => s.scope.startsWith('user:tg-'));
        const global = this.scopes.find((s) => s.scope === 'global');
        this.selectedScope = (telegram || global || this.scopes[0]).scope;
      }
      this.scopeSelect.innerHTML = this.scopes
        .map((s) => `<option value="${escapeHTML(s.scope)}">${escapeHTML(s.label)}</option>`)
        .join('');
      this.scopeSelect.value = this.selectedScope;
    } catch (_err) {
      this.scopeSelect.innerHTML = '';
    }
  }

  async fetchMessages(params = '') {
    const parsed = new URLSearchParams(params);
    const resp = await fetch('/api/chat/messages' + this.chatUrl('', Object.fromEntries(parsed.entries())));
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    return resp.json();
  }

  attachmentsNode(message) {
    const attachments = message.attachments || [];
    if (!attachments.length) return null;
    const wrap = document.createElement('div');
    wrap.className = 'attachments';
    for (const attachment of attachments) {
      const card = document.createElement('a');
      card.className = 'attachment-card';
      card.href = this.chatUrl(
        attachment.download_url || '/api/chat/attachments/' + encodeURIComponent(attachment.id)
      );
      card.target = '_blank';
      card.rel = 'noopener';
      const name = escapeHTML(attachment.original_name || 'attachment');
      const type = escapeHTML(attachment.mime_type || '檔案');
      const size = this.formatBytes(attachment.size_bytes || 0);
      if (attachment.preview_url) {
        const img = document.createElement('img');
        img.className = 'attachment-thumb';
        img.src = this.chatUrl(attachment.preview_url);
        img.alt = attachment.original_name || 'attachment preview';
        card.appendChild(img);
      }
      const meta = document.createElement('span');
      meta.className = 'attachment-meta';
      meta.innerHTML =
        '<strong>' + name + '</strong><small>' + type + ' · ' + escapeHTML(size) + '</small>';
      card.appendChild(meta);
      wrap.appendChild(card);
    }
    return wrap;
  }

  formatBytes(bytes) {
    if (!bytes) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let value = Number(bytes);
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return (unit === 0 ? value.toFixed(0) : value.toFixed(1)) + ' ' + units[unit];
  }

  messageNode(message) {
    const div = document.createElement('div');
    div.className = 'bubble ' + (message.role === 'user' ? 'user' : 'assistant');
    div.dataset.messageId = message.id;
    if (message.role === 'assistant' && message.content_html) {
      div.innerHTML = message.content_html;
    } else {
      div.textContent = message.content;
    }
    if (message.status === 'error') div.classList.add('error');
    const attachments = this.attachmentsNode(message);
    if (attachments) div.appendChild(attachments);
    return div;
  }

  // Collapsible group for an assistant message's helper-baton steps; the steps
  // are fetched lazily the first time the user expands it (most turns have none,
  // so we only attach this when step_count > 0).
  lazyStepsNode(message) {
    const details = document.createElement('details');
    details.className = 'baton-group';
    const label =
      message.step_count > 1 ? '🔍 推理過程（' + message.step_count + ' 棒）' : '🔍 推理過程';
    details.innerHTML =
      '<summary>' + escapeHTML(label) + '</summary><div class="baton-group-body"></div>';
    let loaded = false;
    details.addEventListener('toggle', async () => {
      if (!details.open || loaded) return;
      loaded = true;
      const body = details.querySelector('.baton-group-body');
      body.innerHTML = '<p class="baton-loading">載入中…</p>';
      try {
        const resp = await fetch(
          this.chatUrl('/api/chat/messages/' + encodeURIComponent(message.id) + '/steps')
        );
        if (!resp.ok) throw new Error('HTTP ' + resp.status);
        const steps = await resp.json();
        body.innerHTML = '';
        for (const step of steps) {
          const card = document.createElement('details');
          card.className = 'baton';
          card.open = true;
          // content_html is server-produced safe HTML; fall back to escaped text.
          const inner = step.content_html || escapeHTML(step.content);
          card.innerHTML =
            '<summary>🔍 悟空·' + escapeHTML(step.role) + ' 的產出</summary>' +
            '<div class="baton-body">' + inner + '</div>';
          body.appendChild(card);
          this.enhanceCodeBlocks(card);
        }
      } catch (err) {
        body.innerHTML = '<p class="baton-loading">載入失敗：' + escapeHTML(err.message) + '</p>';
        loaded = false; // allow a retry on next expand
      }
    });
    return details;
  }

  lazyEventsNode(message) {
    const details = document.createElement('details');
    details.className = 'turn-events-group';
    details.innerHTML =
      '<summary>💭 思考與工具紀錄</summary><div class="turn-events-body"></div>';
    let loaded = false;
    details.addEventListener('toggle', async () => {
      if (!details.open || loaded) return;
      loaded = true;
      const body = details.querySelector('.turn-events-body');
      body.innerHTML = '<p class="baton-loading">載入中…</p>';
      try {
        const resp = await fetch(
          this.chatUrl('/api/chat/messages/' + encodeURIComponent(message.id) + '/events')
        );
        if (!resp.ok) throw new Error('HTTP ' + resp.status);
        const events = await resp.json();
        const reasoning = events
          .filter((event) => event.kind === 'reasoning')
          .map((event) => event.content)
          .join('');
        const tools = events.filter((event) => event.kind !== 'reasoning');

        body.innerHTML = '';
        if (reasoning.trim()) {
          const block = document.createElement('details');
          block.className = 'thinking';
          block.open = true;
          block.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
          block.querySelector('.reasoning').textContent = reasoning;
          body.appendChild(block);
        }
        if (tools.length) {
          const list = document.createElement('ol');
          list.className = 'turn-events-timeline';
          for (const event of tools) {
            const item = document.createElement('li');
            if (event.kind === 'tool_use') {
              item.textContent = '使用工具 ' + (event.label || event.content || 'tool');
            } else {
              item.textContent = event.content || event.kind;
            }
            list.appendChild(item);
          }
          body.appendChild(list);
        }
        if (!reasoning.trim() && !tools.length) {
          body.innerHTML = '<p class="baton-loading">沒有紀錄。</p>';
        }
      } catch (err) {
        body.innerHTML = '<p class="baton-loading">載入失敗：' + escapeHTML(err.message) + '</p>';
        loaded = false;
      }
    });
    return details;
  }

  isNearBottom() {
    return this.log.scrollHeight - this.log.scrollTop - this.log.clientHeight < 120;
  }

  scrollToBottom() {
    this.log.scrollTop = this.log.scrollHeight;
  }

  scrollToBottomAfterLayout() {
    requestAnimationFrame(() => this.scrollToBottom());
  }

  maybeScrollToBottom(wasNearBottom) {
    if (wasNearBottom) this.scrollToBottom();
  }

  renderMessages(messages, mode) {
    if (mode !== 'prepend') this.renderedMessageIds.clear();
    const nodes = [];
    let lastDate = null;
    for (const message of messages) {
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
        // Event and steps cards sit above the answer, matching live-turn ordering.
        if (message.event_count > 0) nodes.push(this.lazyEventsNode(message));
        if (message.step_count > 0) nodes.push(this.lazyStepsNode(message));
        this.enhanceCodeBlocks(bubbleNode);
      }
      nodes.push(bubbleNode);
    }
    if (mode === 'prepend') {
      const previousHeight = this.log.scrollHeight;
      for (const node of nodes.reverse()) this.log.prepend(node);
      this.log.scrollTop = this.log.scrollHeight - previousHeight;
    } else {
      this.log.innerHTML = '';
      for (const node of nodes) this.log.appendChild(node);
      this.scrollToBottomAfterLayout();
    }
    this.oldestId = this.log.querySelector('[data-message-id]')?.dataset.messageId || null;
  }

  async loadLatest() {
    try {
      const data = await this.fetchMessages('limit=10');
      if (!data.messages.length) {
        this.log.innerHTML = '<p class="empty-state">還沒有對話，問悟空第一個問題。</p>';
        return;
      }
      this.hasMore = data.has_more;
      this.renderMessages(data.messages, 'replace');
    } catch (err) {
      this.log.innerHTML = '<p class="empty-state">無法讀取對話歷史：' + escapeHTML(err.message) + '</p>';
    }
  }

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

  async jumpToDate() {
    const date = this.querySelector('#jump-date').value;
    if (!date) return;
    try {
      const data = await this.fetchMessages('date=' + encodeURIComponent(date) + '&limit=10');
      this.hasMore = data.has_more;
      if (!data.messages.length) {
        this.log.innerHTML = '<p class="empty-state">這天沒有對話。</p>';
        return;
      }
      this.renderMessages(data.messages, 'replace');
    } catch (err) {
      this.log.innerHTML = '<p class="empty-state">無法跳到指定日期：' + escapeHTML(err.message) + '</p>';
    }
  }

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

  bubble(cls, innerHTML) {
    const div = document.createElement('div');
    div.className = 'bubble ' + cls;
    div.innerHTML = innerHTML;
    this.log.appendChild(div);
    this.log.scrollTop = this.log.scrollHeight;
    return div;
  }

  appendBubble(cls, innerHTML) {
    const div = document.createElement('div');
    div.className = 'bubble ' + cls;
    div.innerHTML = innerHTML;
    this.log.appendChild(div);
    return div;
  }

  parseLiveEvent(ev) {
    try {
      const data = JSON.parse(ev.data);
      if (data.id) this.liveCursor = Math.max(this.liveCursor, Number(data.id));
      return data;
    } catch (_err) {
      return null;
    }
  }

  ensureLiveProgress() {
    if (!this.liveProgress) {
      this.liveProgress = this.appendBubble('status', '🐵 收到，思考中…');
    }
    return this.liveProgress;
  }

  ensureLiveThinking() {
    if (!this.liveThinking) {
      this.liveThinking = document.createElement('details');
      this.liveThinking.className = 'thinking';
      this.liveThinking.open = true;
      this.liveThinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
      this.log.appendChild(this.liveThinking);
    }
    return this.liveThinking;
  }

  handleLiveEvent(ev) {
    const data = this.parseLiveEvent(ev);
    if (!data || data.scope !== this.selectedScope) return;
    const isDuplicate = data.message_id && this.renderedMessageIds.has(String(data.message_id));
    if (isDuplicate && !['answer', 'error'].includes(data.kind)) return;
    const wasNearBottom = this.isNearBottom();

    if (data.kind === 'user') {
      const node = this.appendBubble('user', html`${data.content || ''}`.toString());
      if (data.message_id) {
        node.dataset.messageId = data.message_id;
        this.renderedMessageIds.add(String(data.message_id));
      }
    } else if (data.kind === 'role') {
      this.ensureLiveProgress().innerHTML = '🐵 悟空·' + escapeHTML(data.content || '') + ' 思考中…';
    } else if (data.kind === 'reasoning') {
      const thinking = this.ensureLiveThinking();
      thinking.querySelector('.reasoning').textContent += data.content || '';
    } else if (data.kind === 'tool') {
      const progress = this.ensureLiveProgress();
      progress.innerHTML = '🐵 使用工具 ' + escapeHTML(data.label || data.content || 'tool') + '…';
      const thinking = this.ensureLiveThinking();
      thinking.querySelector('.reasoning').textContent += '\n▸ 使用工具 ' + (data.label || data.content || 'tool') + '\n';
    } else if (data.kind === 'step') {
      const details = document.createElement('details');
      details.className = 'baton';
      details.innerHTML =
        '<summary>🔍 悟空·' + escapeHTML(data.label || 'step') + ' 的產出</summary>' +
        '<div class="baton-body">' + (data.content || '') + '</div>';
      this.log.appendChild(details);
      this.enhanceCodeBlocks(details);
    } else if (data.kind === 'answer') {
      if (this.liveProgress) this.liveProgress.remove();
      this.liveProgress = null;
      if (!isDuplicate) {
        const div = this.appendBubble('assistant', unsafe(data.content_html || data.content || '').toString());
        if (data.message_id) {
          div.dataset.messageId = data.message_id;
          this.renderedMessageIds.add(String(data.message_id));
        }
        this.enhanceCodeBlocks(div);
      }
      this.liveThinking = null;
    } else if (data.kind === 'error') {
      if (this.liveProgress) this.liveProgress.remove();
      this.liveProgress = null;
      if (!isDuplicate) {
        const div = this.appendBubble('assistant', '⚠️ ' + escapeHTML(data.content || '處理失敗'));
        if (data.message_id) {
          div.dataset.messageId = data.message_id;
          this.renderedMessageIds.add(String(data.message_id));
        }
      }
      this.liveThinking = null;
    }

    this.maybeScrollToBottom(wasNearBottom);
  }

  send() {
    const text = this.input.value.trim();
    if (!text) return;
    this.input.value = '';
    this.input.style.height = 'auto';
    if (this.log.querySelector('.empty-state')) this.log.innerHTML = '';
    // User bubble: input is escaped via the html`` template.
    this.bubble('user', html`${text}`.toString());
    // Single progress bubble, updated in place by role events.
    const progress = this.bubble('status', '🐵 收到，思考中…');
    let thinking = null;

    const es = new EventSource(this.chatUrl('/chat', { q: text }));

    es.addEventListener('role', (ev) => {
      progress.innerHTML = '🐵 悟空·' + escapeHTML(ev.data) + ' 思考中…';
    });
    es.addEventListener('reasoning', (ev) => {
      if (!thinking) {
        thinking = document.createElement('details');
        thinking.className = 'thinking';
        thinking.open = true;
        thinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
        this.log.appendChild(thinking);
      }
      thinking.querySelector('.reasoning').textContent += ev.data;
      this.log.scrollTop = this.log.scrollHeight;
    });
    es.addEventListener('tool', (ev) => {
      progress.innerHTML = '🐵 使用工具 ' + escapeHTML(ev.data) + '…';
      if (!thinking) {
        thinking = document.createElement('details');
        thinking.className = 'thinking';
        thinking.open = true;
        thinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
        this.log.appendChild(thinking);
      }
      thinking.querySelector('.reasoning').textContent += '\n▸ 使用工具 ' + ev.data + '\n';
      this.log.scrollTop = this.log.scrollHeight;
    });
    es.addEventListener('step', (ev) => {
      // Helper-baton output: a collapsed, visually-secondary card above the answer.
      let role = '', skill = '', stepHtml = '';
      try {
        const parsed = JSON.parse(ev.data);
        role = parsed.role || '';
        skill = parsed.skill || '';
        stepHtml = parsed.html || '';
      } catch (_err) {
        return;
      }
      const details = document.createElement('details');
      details.className = 'baton';
      // role is escaped; stepHtml is server-produced safe HTML, trusted as-is.
      const label = skill ? role + ' + ' + skill : role;
      details.innerHTML =
        '<summary>🔍 悟空·' + escapeHTML(label) + ' 的產出</summary>' +
        '<div class="baton-body">' + stepHtml + '</div>';
      this.log.appendChild(details);
      this.enhanceCodeBlocks(details);
      this.log.scrollTop = this.log.scrollHeight;
    });
    es.addEventListener('answer', (ev) => {
      progress.remove();
      // Server already produced safe HTML; mark it trusted.
      const div = this.bubble('assistant', unsafe(ev.data).toString());
      this.enhanceCodeBlocks(div);
    });
    es.addEventListener('error', (ev) => {
      // EventSource also fires a data-less 'error' on connection close; ignore
      // those and only surface server-sent error events (which carry data).
      if (!ev.data) return;
      progress.remove();
      this.bubble('assistant', '⚠️ ' + escapeHTML(ev.data));
      es.close();
    });
    es.addEventListener('done', () => {
      progress.remove();
      es.close();
    });
  }
}
