import { html, unsafe, escapeHTML } from '/lib/html.js';

// <wukong-chat>: message log + composer + SSE wiring. Self-contained custom
// element (no router/services), per plainvanillaweb core conventions.
export class WukongChat extends HTMLElement {
  connectedCallback() {
    this.scopes = [];
    this.selectedScope = '';
    this.innerHTML = html`
      <div class="chat-toolbar">
        <label class="chat-source">來源 <select id="chat-scope"></select></label>
        <label>跳到日期 <input id="jump-date" type="date" /></label>
        <button id="jump-button" type="button">前往</button>
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
      this.selectedScope = this.scopeSelect.value;
      this.resetMessages();
      this.loadLatest();
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
    await this.loadScopes();
    await this.loadLatest();
  }

  resetMessages() {
    this.hasMore = false;
    this.oldestId = null;
    this.log.innerHTML = '';
  }

  async loadScopes() {
    try {
      const resp = await fetch('/api/chat/scopes' + this.tokenParam('?'));
      if (!resp.ok) return;
      this.scopes = await resp.json();
      if (!this.selectedScope && this.scopes.length > 0) {
        const global = this.scopes.find((s) => s.scope === 'global');
        this.selectedScope = (global || this.scopes[0]).scope;
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
    return div;
  }

  renderMessages(messages, mode) {
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
      if (message.role === 'assistant') {
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
      this.log.scrollTop = this.log.scrollHeight;
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
        thinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
        this.log.appendChild(thinking);
      }
      thinking.querySelector('.reasoning').textContent += ev.data;
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
