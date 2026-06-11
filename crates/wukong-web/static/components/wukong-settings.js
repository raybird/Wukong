import { html } from '/lib/html.js';

export class WukongSettings extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="settings-card">
        <h2>Telegram 設定</h2>
        <p class="settings-help">輸入 Bot token 與允許的 chat/user ID。儲存後 Telegram 服務會自動開始等待訊息。</p>
        <form id="settings-form" class="settings-form">
          <label>Bot token<input id="tg-token" type="password" autocomplete="off" placeholder="123456:ABC..." /></label>
          <label>Allowed IDs<textarea id="tg-allowed" rows="3" placeholder="例如：123456789 或多個 ID 以空白分隔"></textarea></label>
          <button type="submit">儲存設定</button>
        </form>
        <p id="settings-status" class="settings-status">載入中…</p>
      </section>
    `.toString();
    this.status = this.querySelector('#settings-status');
    this.tokenInput = this.querySelector('#tg-token');
    this.allowedInput = this.querySelector('#tg-allowed');
    this.querySelector('#settings-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.save();
    });
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/settings' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.allowedInput.value = data.telegram.allowed || '';
    this.status.textContent = data.telegram.configured
      ? '已設定 token：' + data.telegram.token
      : '尚未設定 Telegram token';
  }

  async save() {
    const body = {
      telegram: {
        token: this.tokenInput.value.trim(),
        allowed: this.allowedInput.value.trim(),
      },
    };
    const resp = await fetch('/api/settings' + this.tokenParam(), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!resp.ok) {
      this.status.textContent = '儲存失敗：HTTP ' + resp.status;
      return;
    }
    this.tokenInput.value = '';
    this.status.textContent = '已儲存。Telegram 服務會自動套用設定。';
    await this.load();
  }
}
