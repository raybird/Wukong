import { html, escapeHTML } from '/lib/html.js';

export class WukongSchedules extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>排程</h2><div id="schedules-list">載入中…</div></section>`.toString();
    this.list = this.querySelector('#schedules-list');
    this.load();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/schedules' + this.tokenParam());
    if (!resp.ok) {
      this.list.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取排程：HTTP ' + resp.status;
      return;
    }
    const jobs = await resp.json();
    if (!jobs.length) {
      this.list.innerHTML = '<p class="empty-state">目前沒有排程，可先用 CLI 建立排程。</p>';
      return;
    }
    this.list.innerHTML = jobs.map((job) => this.card(job)).join('');
    this.list.querySelectorAll('[data-action]').forEach((button) => {
      button.addEventListener('click', () => this.act(button.dataset.id, button.dataset.action));
    });
  }

  card(job) {
    const next = job.next_run_at ? new Date(job.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    const last = job.last_run_at ? new Date(job.last_run_at * 1000).toLocaleString('zh-TW') : '尚未執行';
    const detail = job.kind === 'turn' ? job.prompt : job.task;
    const toggle = job.enabled ? 'disable' : 'enable';
    const toggleLabel = job.enabled ? '停用' : '啟用';
    return `
      <article class="schedule-card">
        <h3>${escapeHTML(job.name)}</h3>
        <p>類型：${escapeHTML(job.kind)} / ${escapeHTML(job.scope || 'global')}</p>
        <p>內容：${escapeHTML(detail || '')}</p>
        <p>Cron：<code>${escapeHTML(job.cron)}</code></p>
        <p>狀態：${job.enabled ? '啟用' : '停用'}</p>
        <p>下次：${escapeHTML(next)}</p>
        <p>上次：${escapeHTML(last)}</p>
        <div class="schedule-actions">
          <button data-id="${escapeHTML(job.id)}" data-action="${toggle}">${toggleLabel}</button>
          <button data-id="${escapeHTML(job.id)}" data-action="delete">刪除</button>
        </div>
      </article>
    `;
  }

  async act(id, action) {
    if (action === 'delete' && !confirm('確定要刪除這個排程？')) return;
    const method = action === 'delete' ? 'DELETE' : 'POST';
    const path = action === 'delete'
      ? '/api/schedules/' + encodeURIComponent(id)
      : '/api/schedules/' + encodeURIComponent(id) + '/' + action;
    const resp = await fetch(path + this.tokenParam(), { method });
    if (!resp.ok) {
      alert(resp.status === 404 ? '找不到排程。' : '操作失敗：HTTP ' + resp.status);
      return;
    }
    await this.load();
  }
}
