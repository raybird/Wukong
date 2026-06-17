import { html } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>系統</h2><p class="settings-help">載入中…</p></section>`.toString();
  }
}
