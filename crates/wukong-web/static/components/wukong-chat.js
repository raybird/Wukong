import { html, unsafe, escapeHTML } from '/lib/html.js';

// <wukong-chat>: message log + composer + SSE wiring. Self-contained custom
// element (no router/services), per plainvanillaweb core conventions.
export class WukongChat extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <div class="log" id="log"></div>
      <form id="form" class="composer">
        <input id="q" type="text" autocomplete="off" placeholder="問悟空…" />
        <button type="submit">送出</button>
      </form>
    `.toString();
    this.log = this.querySelector('#log');
    this.input = this.querySelector('#q');
    this.querySelector('#form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.send();
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
