import { html } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>系統</h2><div id="system-summary">載入中…</div></section>`.toString();
    this.summary = this.querySelector('#system-summary');
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/system' + this.tokenParam());
    if (!resp.ok) {
      this.summary.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取系統資訊：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    const next = data.next_run_at ? new Date(data.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    this.summary.innerHTML = html`
      <dl class="system-list">
        <dt>Scope</dt><dd>${data.scope}</dd>
        <dt>Web token</dt><dd>${data.token_enabled ? '已啟用' : '未啟用'}</dd>
        <dt>Memory DB</dt><dd>${data.memory_db}</dd>
        <dt>排程總數</dt><dd>${data.schedule_total}</dd>
        <dt>啟用排程</dt><dd>${data.schedule_enabled}</dd>
        <dt>最近下次執行</dt><dd>${next}</dd>
      </dl>
    `.toString();
  }
}
