import { escapeHTML } from '/lib/html.js';

export function dateSeparatorNode(text) {
  const sep = document.createElement('div');
  sep.className = 'date-separator';
  sep.textContent = text;
  return sep;
}

export function unreadDividerNode() {
  const div = document.createElement('div');
  div.className = 'unread-divider';
  div.textContent = '以下是上次離開後的新紀錄';
  return div;
}

export function messageFrameNode(message, { attachmentsNode } = {}) {
  const frame = document.createElement('article');
  const role = message.role === 'user' ? 'user' : 'assistant';
  frame.className = 'message-frame ' + role;
  frame.dataset.messageId = message.id;
  frame.dataset.role = role;

  const meta = document.createElement('header');
  meta.className = 'message-meta';
  const label = role === 'user' ? '你' : '悟空';
  meta.innerHTML = '<span>' + escapeHTML(label) + '</span>';
  frame.appendChild(meta);

  const body = document.createElement('div');
  body.className = 'message-content bubble ' + role;
  if (message.role === 'assistant' && message.content_html) body.innerHTML = message.content_html;
  else body.textContent = message.content || '';
  if (message.status === 'error') body.classList.add('error');
  frame.appendChild(body);

  const attachments = attachmentsNode ? attachmentsNode(message) : null;
  if (attachments) frame.appendChild(attachments);
  return { frame, body };
}

export function bubbleNode(cls, innerHTML) {
  const div = document.createElement('div');
  div.className = 'bubble ' + cls;
  div.innerHTML = innerHTML;
  return div;
}
