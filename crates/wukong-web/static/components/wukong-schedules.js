import { html } from '/lib/html.js';

export class WukongSchedules extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>排程</h2><p class="settings-help">載入中…</p></section>`.toString();
  }
}
