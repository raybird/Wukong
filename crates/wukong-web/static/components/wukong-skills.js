import { html } from '/lib/html.js';

export class WukongSkills extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>技能</h2>
            <p class="panel-help">Phase 1 先顯示角色與 Superpowers catalog；偏好儲存與 planner 注入留到 Phase 2。</p>
          </div>
        </div>
        <div id="skills-status" class="settings-status">載入中…</div>
        <section class="control-card"><h3>角色</h3><div id="roles" class="control-row"></div></section>
        <section><h3>Superpowers</h3><div id="skills" class="skill-grid"></div></section>
      </section>
    `.toString();
    this.status = this.querySelector('#skills-status');
    this.roles = this.querySelector('#roles');
    this.skills = this.querySelector('#skills');
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/skills/catalog' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取技能目錄：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入技能目錄';
    this.roles.innerHTML = (data.roles || []).map((role) => '<span class="tag">' + role.name + '</span>').join('');
    this.skills.innerHTML = (data.skills || []).map((skill) => html`
      <article class="skill-card">
        <h3>${skill.name}</h3>
        <p>${skill.description}</p>
        <p><span class="tag">主責 ${skill.primary_role}</span> ${skill.collaborator_role ? '<span class="tag">協作 ' + skill.collaborator_role + '</span>' : ''}</p>
      </article>
    `.toString()).join('');
  }
}
