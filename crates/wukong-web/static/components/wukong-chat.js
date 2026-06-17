import { html, unsafe, escapeHTML } from '/lib/html.js';

// <wukong-chat>: message log + composer + SSE wiring. Self-contained custom
// element (no router/services), per plainvanillaweb core conventions.
export class WukongChat extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <div class="chat-toolbar">
        <label>跳到日期 <input id="jump-date" type="date" /></label>
        <button id="jump-button" type="button">前往</button>
      </div>
      <div class="log" id="log"></div>
      <form id="form" class="composer">
        <input id="q" type="text" autocomplete="off" placeholder="問悟空…" />
        <button type="submit">送出</button>
      </form>
    `.toString();
    this.log = this.querySelector('#log');
    this.input = this.querySelector('#q');
    this.loadingOlder = false;
    this.hasMore = false;
    this.oldestId = null;
    this.querySelector('#form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.send();
    });
    this.querySelector('#jump-button').addEventListener('click', () => this.jumpToDate());
    this.log.addEventListener('scroll', () => {
      if (this.log.scrollTop < 80) this.loadOlder();
    });
    this.loadLatest();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async fetchMessages(params = '') {
    const token = this.tokenParam(params ? '&' : '?');
    const resp = await fetch('/api/chat/messages' + (params ? '?' + params : '') + token);
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
      nodes.push(this.messageNode(message));
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
    if (this.log.querySelector('.empty-state')) this.log.innerHTML = '';
    // User bubble: input is escaped via the html`` template.
    this.bubble('user', html`${text}`.toString());
    // Single progress bubble, updated in place by role events.
    const progress = this.bubble('status', '🐵 收到，思考中…');
    let thinking = null;

    const tokenParam = window.WUKONG_TOKEN
      ? '&token=' + encodeURIComponent(window.WUKONG_TOKEN)
      : '';
    const es = new EventSource('/chat?q=' + encodeURIComponent(text) + tokenParam);

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
      this.bubble('assistant', unsafe(ev.data).toString());
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
