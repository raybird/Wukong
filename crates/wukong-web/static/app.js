import { WukongChat } from '/components/wukong-chat.js';
import { WukongMemory } from '/components/wukong-memory.js';
import { WukongSkills } from '/components/wukong-skills.js';
import { WukongSettings } from '/components/wukong-settings.js';
import { WukongSchedules } from '/components/wukong-schedules.js';
import { WukongSystem } from '/components/wukong-system.js';

customElements.define('wukong-chat', WukongChat);
customElements.define('wukong-memory', WukongMemory);
customElements.define('wukong-skills', WukongSkills);
customElements.define('wukong-settings', WukongSettings);
customElements.define('wukong-schedules', WukongSchedules);
customElements.define('wukong-system', WukongSystem);

const app = document.querySelector('#app');

const routes = {
  '#/chat': '<wukong-chat></wukong-chat>',
  '#/memory': '<wukong-memory></wukong-memory>',
  '#/skills': '<wukong-skills></wukong-skills>',
  '#/schedules': '<wukong-schedules></wukong-schedules>',
  '#/system': '<wukong-system></wukong-system>',
  '#/settings': '<wukong-settings></wukong-settings>',
};

function render() {
  const route = window.location.hash || '#/chat';
  app.innerHTML = routes[route] || '<section class="empty-state"><h2>找不到頁面</h2><p><a href="#/chat">回到對話</a></p></section>';
  document.querySelectorAll('header nav a').forEach((a) => {
    a.classList.toggle('active', route === '#/' + a.dataset.route);
  });
}

window.addEventListener('hashchange', render);
if (!window.location.hash) window.location.hash = '#/chat';
render();
