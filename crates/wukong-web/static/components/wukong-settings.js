import { html } from '/lib/html.js';

export class WukongSettings extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>設定</h2>
            <p class="panel-help">可持久化設定。System 分頁保留給只讀診斷。</p>
          </div>
        </div>
        <section class="settings-card">
          <h3>全域模型</h3>
          <p class="settings-help">第一版只支援全域預設模型，後續 Web / Telegram / Scheduler / CLI turns 都會使用。</p>
          <form id="model-form" class="settings-form">
            <label>Default model<input id="model-input" type="text" placeholder="opencode/deepseek-v4-flash-free" /></label>
            <button type="submit">儲存模型</button>
          </form>
          <p id="model-status" class="settings-status">載入中…</p>
        </section>
        <section class="settings-card">
          <h3>Telegram 設定</h3>
          <p class="settings-help">輸入 Bot token 與允許的 chat/user ID。儲存後 Telegram 服務會自動開始等待訊息。</p>
          <form id="settings-form" class="settings-form">
            <label>Bot token<input id="tg-token" type="password" autocomplete="off" placeholder="123456:ABC..." /></label>
            <label>Allowed IDs<textarea id="tg-allowed" rows="3" placeholder="例如：123456789 或多個 ID 以空白分隔"></textarea></label>
            <button type="submit">儲存 Telegram</button>
          </form>
          <p id="settings-status" class="settings-status">載入中…</p>
        </section>
      </section>
    `.toString();
    this.status = this.querySelector('#settings-status');
    this.modelStatus = this.querySelector('#model-status');
    this.modelInput = this.querySelector('#model-input');
    this.tokenInput = this.querySelector('#tg-token');
    this.allowedInput = this.querySelector('#tg-allowed');
    this.querySelector('#settings-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.saveTelegram();
    });
    this.querySelector('#model-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.saveModel();
    });
    this.loadTelegram();
    this.loadModel();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async loadModel() {
    const resp = await fetch('/api/settings/model' + this.tokenParam());
    if (!resp.ok) {
      this.modelStatus.textContent = '無法讀取模型設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.modelInput.value = data.model || '';
    this.modelInput.disabled = !data.editable;
    this.querySelector('#model-form button').disabled = !data.editable;
    this.modelStatus.textContent = data.model
      ? '目前模型：' + data.model + '（來源：' + data.source + '）'
      : '尚未設定全域模型，將使用底層 agent 預設。';
  }

  async saveModel() {
    const model = this.modelInput.value.trim();
    if (!model) {
      this.modelStatus.textContent = '模型不可為空。';
      return;
    }
    const resp = await fetch('/api/settings/model' + this.tokenParam(), {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ model }),
    });
    if (!resp.ok) {
      this.modelStatus.textContent = '儲存模型失敗：HTTP ' + resp.status;
      return;
    }
    await this.loadModel();
  }

  async loadTelegram() {
    const resp = await fetch('/api/settings' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取 Telegram 設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.allowedInput.value = data.telegram.allowed || '';
    this.status.textContent = data.telegram.configured
      ? '已設定 token：' + data.telegram.token
      : '尚未設定 Telegram token';
  }

  async saveTelegram() {
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
    await this.loadTelegram();
  }
}
