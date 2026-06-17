import { WukongChat } from '/components/wukong-chat.js';
import { WukongSettings } from '/components/wukong-settings.js';
import { WukongSchedules } from '/components/wukong-schedules.js';
import { WukongSystem } from '/components/wukong-system.js';
import { html } from '/lib/html.js';

customElements.define('wukong-chat', WukongChat);
customElements.define('wukong-settings', WukongSettings);
customElements.define('wukong-schedules', WukongSchedules);
customElements.define('wukong-system', WukongSystem);

const app = document.querySelector('#app');

function settingsShell(active, tag) {
  return html`
    <section class="settings-layout">
      <nav class="settings-tabs">
        <a class="${active === 'telegram' ? 'active' : ''}" href="#/settings/telegram">Telegram</a>
        <a class="${active === 'system' ? 'active' : ''}" href="#/settings/system">系統</a>
        <a class="${active === 'schedules' ? 'active' : ''}" href="#/settings/schedules">排程</a>
      </nav>
      <div class="settings-outlet">${tag}</div>
    </section>
  `.toString();
}

function render() {
  const route = window.location.hash || '#/chat';
  if (route === '#/settings') {
    window.location.hash = '#/settings/telegram';
    return;
  }
  if (route === '#/chat') {
    app.innerHTML = '<wukong-chat></wukong-chat>';
  } else if (route === '#/settings/telegram') {
    app.innerHTML = settingsShell('telegram', '<wukong-settings></wukong-settings>');
  } else if (route === '#/settings/system') {
    app.innerHTML = settingsShell('system', '<wukong-system></wukong-system>');
  } else if (route === '#/settings/schedules') {
    app.innerHTML = settingsShell('schedules', '<wukong-schedules></wukong-schedules>');
  } else {
    app.innerHTML = '<section class="empty-state"><h2>找不到頁面</h2><p><a href="#/chat">回到對話</a></p></section>';
  }
  document.querySelectorAll('header nav a').forEach((a) => {
    const key = a.dataset.route;
    a.classList.toggle('active', route.includes(key));
  });
}

window.addEventListener('hashchange', render);
if (!window.location.hash) window.location.hash = '#/chat';
render();
