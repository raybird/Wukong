import { escapeHTML } from '/lib/html.js';

export function activityDetailsNode({ className, summary, loadingText }) {
  const details = document.createElement('details');
  details.className = 'activity-card ' + className;
  details.innerHTML = '<summary>' + escapeHTML(summary) + '</summary><div class="activity-card-body"></div>';
  details.querySelector('.activity-card-body').innerHTML = '<p class="baton-loading">' + escapeHTML(loadingText) + '</p>';
  return details;
}

export function liveThinkingNode() {
  const details = document.createElement('details');
  details.className = 'activity-card thinking';
  details.open = true;
  details.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
  return details;
}

export function activityRailNode() {
  const rail = document.createElement('div');
  rail.className = 'activity-rail';
  return rail;
}
