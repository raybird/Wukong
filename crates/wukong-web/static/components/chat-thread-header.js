import { html } from '/lib/html.js';

export function threadHeaderTemplate() {
  return html`
    <section class="chat-thread-header" aria-label="對話工作台狀態">
      <div class="thread-source">
        <label class="chat-source">來源 <select id="chat-scope"></select></label>
      </div>
      <div class="thread-jump">
        <label>跳到日期 <input id="jump-date" type="date" /></label>
        <button id="jump-button" type="button">前往</button>
      </div>
      <div class="thread-status" aria-live="polite">
        <span id="chat-model-status" class="tag">模型：載入中</span>
        <span id="chat-skill-status" class="tag">技能偏好：Phase 2</span>
      </div>
    </section>
  `.toString();
}
