import { escapeHTML } from '/lib/html.js';

export function questionCardNode(request, source, { onSubmit, onReject }) {
  if (!request || !request.request_id || !request.session_id || !Array.isArray(request.questions)) return null;
  const state = {
    tab: 0,
    answers: request.questions.map(() => []),
    custom: request.questions.map(() => ''),
    sending: false,
  };
  const card = document.createElement('section');
  card.className = 'question-card activity-card';
  card.dataset.requestId = request.request_id;
  card.dataset.source = source || '';

  const finish = (text) => {
    card.classList.add('question-card-done');
    card.innerHTML = '<div class="question-done">' + escapeHTML(text) + '</div>';
  };

  const setStatus = (text, cls = '') => {
    const status = card.querySelector('.question-status');
    if (!status) return;
    status.textContent = text;
    status.className = 'question-status ' + cls;
  };

  const submit = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('送出中…');
    try {
      await onSubmit(request, state.answers);
      finish('已送出回答。');
    } catch (err) {
      state.sending = false;
      setStatus('送出失敗：' + err.message, 'error');
    }
  };

  const reject = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('取消中…');
    try {
      await onReject(request);
      finish('已取消問題。');
    } catch (err) {
      state.sending = false;
      setStatus('取消失敗：' + err.message, 'error');
    }
  };

  const render = () => {
    const question = request.questions[state.tab];
    if (!question) return;
    const selected = state.answers[state.tab] || [];
    const isLast = state.tab >= request.questions.length - 1;
    card.innerHTML = '<div class="question-title">問題 ' + (state.tab + 1) + ' / ' + request.questions.length + '</div>';
    if (question.header) card.innerHTML += '<div class="question-header">' + escapeHTML(question.header) + '</div>';
    card.innerHTML += '<div class="question-text">' + escapeHTML(question.question || '') + '</div>';
    const options = document.createElement('div');
    options.className = 'question-options';
    for (const option of question.options || []) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'question-option';
      const picked = selected.includes(option.label);
      if (picked) button.classList.add('picked');
      button.innerHTML = '<span>' + escapeHTML(picked ? '✓ ' : '') + escapeHTML(option.label || '') + '</span>' +
        (option.description ? '<small>' + escapeHTML(option.description) + '</small>' : '');
      button.addEventListener('click', () => {
        if (question.multiple) {
          state.answers[state.tab] = picked
            ? selected.filter((item) => item !== option.label)
            : [...selected, option.label];
          render();
          return;
        }
        state.answers[state.tab] = [option.label];
        if (isLast) void submit();
        else {
          state.tab += 1;
          render();
        }
      });
      options.appendChild(button);
    }
    card.appendChild(options);
    if (question.custom) {
      const custom = document.createElement('textarea');
      custom.className = 'question-custom';
      custom.rows = 2;
      custom.placeholder = '自訂回答…';
      custom.value = state.custom[state.tab] || '';
      custom.addEventListener('input', () => {
        state.custom[state.tab] = custom.value;
      });
      card.appendChild(custom);
    }
    const status = document.createElement('div');
    status.className = 'question-status';
    card.appendChild(status);
    const footer = document.createElement('div');
    footer.className = 'question-footer';
    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = '取消';
    cancel.addEventListener('click', () => void reject());
    footer.appendChild(cancel);
    if (!isLast) {
      const next = document.createElement('button');
      next.type = 'button';
      next.textContent = '下一題';
      next.addEventListener('click', () => {
        const custom = (state.custom[state.tab] || '').trim();
        if (custom) {
          state.answers[state.tab] = question.multiple
            ? Array.from(new Set([...(state.answers[state.tab] || []), custom]))
            : [custom];
        }
        state.tab += 1;
        render();
      });
      footer.appendChild(next);
    } else {
      const send = document.createElement('button');
      send.type = 'button';
      send.textContent = '送出';
      send.addEventListener('click', () => {
        const custom = (state.custom[state.tab] || '').trim();
        if (custom) {
          state.answers[state.tab] = question.multiple
            ? Array.from(new Set([...(state.answers[state.tab] || []), custom]))
            : [custom];
        }
        void submit();
      });
      footer.appendChild(send);
    }
    card.appendChild(footer);
  };

  render();
  return card;
}
