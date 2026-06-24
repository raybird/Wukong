import { html, unsafe } from '/lib/html.js';

export class WukongSkills extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>技能</h2>
            <p class="panel-help">設定全域角色與 Superpowers 偏好。這些是 planner guidance，不會硬性限制悟空改選更適合的角色或技能。</p>
          </div>
        </div>
        <div id="skills-status" class="settings-status">載入中…</div>
        <section class="control-card">
          <h3>全域技能偏好</h3>
          <label class="control-row"><input id="skills-enabled" type="checkbox"> 啟用偏好提示</label>
          <p class="panel-help">未勾選時，planner 會依照任務自動選擇，不加入偏好提示。</p>
          <button id="skills-save" type="button">儲存偏好</button>
        </section>
        <section class="control-card"><h3>偏好角色</h3><div id="roles" class="control-row"></div></section>
        <section><h3>Superpowers</h3><div id="skills" class="skill-grid"></div></section>
      </section>
    `.toString();
    this.status = this.querySelector('#skills-status');
    this.enabled = this.querySelector('#skills-enabled');
    this.saveButton = this.querySelector('#skills-save');
    this.roles = this.querySelector('#roles');
    this.skills = this.querySelector('#skills');
    this.saveButton.addEventListener('click', () => this.save());
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const [catalogResp, prefsResp] = await Promise.all([
      fetch('/api/skills/catalog' + this.tokenParam()),
      fetch('/api/skills/preferences' + this.tokenParam()),
    ]);
    if (!catalogResp.ok) {
      this.status.textContent = '無法讀取技能目錄：HTTP ' + catalogResp.status;
      return;
    }
    if (!prefsResp.ok) {
      this.status.textContent = '無法讀取技能偏好：HTTP ' + prefsResp.status;
      return;
    }
    this.catalog = await catalogResp.json();
    const prefs = await prefsResp.json();
    this.enabled.checked = !!prefs.enabled;
    this.render(prefs);
    this.status.textContent = '已載入技能偏好';
  }

  render(prefs) {
    const selectedRoles = new Set(prefs.roles || []);
    const selectedSkills = new Set(prefs.skills || []);
    this.roles.innerHTML = (this.catalog.roles || []).map((role) => {
      const id = role.name.toLowerCase();
      return html`
        <label class="tag"><input type="checkbox" name="role" value="${id}" ${selectedRoles.has(id) ? unsafe('checked') : ''}> ${role.name}</label>
      `.toString();
    }).join('');
    this.skills.innerHTML = (this.catalog.skills || []).map((skill) => html`
      <article class="skill-card">
        <label><input type="checkbox" name="skill" value="${skill.name}" ${selectedSkills.has(skill.name) ? unsafe('checked') : ''}> <strong>${skill.name}</strong></label>
        <p>${skill.description}</p>
        <p><span class="tag">主責 ${skill.primary_role}</span> ${skill.collaborator_role ? unsafe('<span class="tag">協作 ' + skill.collaborator_role + '</span>') : ''}</p>
      </article>
    `.toString()).join('');
  }

  selectedValues(name) {
    return [...this.querySelectorAll('input[name="' + name + '"]:checked')].map((input) => input.value);
  }

  async save() {
    this.status.textContent = '儲存中…';
    const resp = await fetch('/api/skills/preferences' + this.tokenParam(), {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        enabled: this.enabled.checked,
        roles: this.selectedValues('role'),
        skills: this.selectedValues('skill'),
      }),
    });
    if (!resp.ok) {
      this.status.textContent = '儲存失敗：HTTP ' + resp.status;
      return;
    }
    const prefs = await resp.json();
    this.render(prefs);
    this.status.textContent = '已儲存技能偏好';
  }
}
