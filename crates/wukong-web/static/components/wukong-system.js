import { html, unsafe } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>系統</h2>
            <p class="panel-help">Read-only runtime diagnostics for backend reachability, tools, environment, and schedules.</p>
          </div>
          <button id="refresh-system" type="button">重新整理</button>
        </div>
        <div id="system-status" class="settings-status">載入中…</div>
        <div id="system-summary" class="stat-grid"></div>
        <div id="system-groups" class="record-list"></div>
      </section>
    `.toString();
    this.status = this.querySelector('#system-status');
    this.summary = this.querySelector('#system-summary');
    this.groups = this.querySelector('#system-groups');
    this.querySelector('#refresh-system').addEventListener('click', () => this.load());
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    this.status.textContent = '載入中…';
    const resp = await fetch('/api/system' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取系統資訊：HTTP ' + resp.status;
      this.summary.innerHTML = '';
      this.groups.innerHTML = '';
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入系統診斷';
    this.renderSummary(data);
    this.renderGroups(data.groups || []);
  }

  renderSummary(data) {
    const next = data.next_run_at ? new Date(data.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    this.summary.innerHTML = html`
      <article class="stat-card"><span>Scope</span><strong>${data.scope}</strong></article>
      <article class="stat-card"><span>Web token</span><strong>${data.token_enabled ? '已啟用' : '未啟用'}</strong></article>
      <article class="stat-card"><span>Memory DB</span><strong>${data.memory_db}</strong></article>
      <article class="stat-card"><span>排程</span><strong>${data.schedule_enabled}/${data.schedule_total}</strong></article>
      <article class="stat-card"><span>最近下次執行</span><strong>${next}</strong></article>
    `.toString();
  }

  renderGroups(groups) {
    this.groups.innerHTML = groups.map((group) => html`
      <section class="control-card">
        <h3>${group.title}</h3>
        <div class="record-list">
          ${(group.items || []).map((item) => unsafe(this.renderItem(item)))}
        </div>
      </section>
    `.toString()).join('') || '<p class="empty-state">沒有診斷資料。</p>';
  }

  renderItem(item) {
    return html`
      <article class="record-card system-diagnostic ${this.statusClass(item.status)}">
        <div><span class="tag">${item.status || 'unknown'}</span> <strong>${item.label}</strong></div>
        <p>${item.summary || ''}</p>
        ${item.detail ? html`<small>${item.detail}</small>` : ''}
        ${item.suggestion ? html`<small>建議：${item.suggestion}</small>` : ''}
      </article>
    `.toString();
  }

  statusClass(status) {
    if (status === 'ok') return 'system-ok';
    if (status === 'warn') return 'system-warn';
    if (status === 'error') return 'system-error';
    return 'system-unknown';
  }
}
