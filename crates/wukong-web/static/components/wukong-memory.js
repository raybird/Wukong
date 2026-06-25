import { html, unsafe } from '/lib/html.js';

export class WukongMemory extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>記憶</h2>
            <p class="panel-help">先提供可觀測能力：健康快照、scope 分布與近期記憶。</p>
          </div>
          <button id="refresh-memory">重新整理</button>
        </div>
        <div id="memory-status" class="settings-status">載入中…</div>
        <div id="memory-summary" class="stat-grid"></div>
        <section class="control-card">
          <div class="control-row">
            <label>Scope <select id="memory-scope"><option value="">全部</option></select></label>
            <label>Kind
              <select id="memory-kind">
                <option value="">全部</option>
                <option value="decision">Decision</option>
                <option value="event">Event</option>
                <option value="skill">Skill</option>
                <option value="note">Note</option>
                <option value="summary">Summary</option>
              </select>
            </label>
          </div>
        </section>
        <section class="control-card">
          <h3>Recall 查詢</h3>
          <p class="panel-help">只讀查詢：顯示 Wukong 會從目前 scope 想起哪些記憶，不會修改記憶資料。</p>
          <div class="control-row">
            <label>Top K <input id="recall-top-k" type="number" min="1" max="20" value="8"></label>
            <span class="tag">mode hybrid</span>
          </div>
          <textarea id="recall-query" rows="3" placeholder="輸入要查詢的記憶線索…"></textarea>
          <div class="control-row">
            <button id="run-recall" type="button">查詢記憶</button>
          </div>
          <div id="recall-status" class="settings-status"></div>
          <div id="recall-results" class="record-list"></div>
        </section>
        <div id="memory-records" class="record-list"></div>
      </section>
    `.toString();
    this.status = this.querySelector('#memory-status');
    this.summary = this.querySelector('#memory-summary');
    this.records = this.querySelector('#memory-records');
    this.scopeSelect = this.querySelector('#memory-scope');
    this.kindSelect = this.querySelector('#memory-kind');
    this.recallQuery = this.querySelector('#recall-query');
    this.recallTopK = this.querySelector('#recall-top-k');
    this.recallStatus = this.querySelector('#recall-status');
    this.recallResults = this.querySelector('#recall-results');
    this.querySelector('#refresh-memory').addEventListener('click', () => this.load());
    this.querySelector('#run-recall').addEventListener('click', () => this.runRecall());
    this.scopeSelect.addEventListener('change', () => this.loadRecords());
    this.kindSelect.addEventListener('change', () => this.loadRecords());
    this.load();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/memory/summary' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取記憶摘要：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入記憶摘要';
    this.renderSummary(data);
    this.renderScopes(data.by_scope || []);
    await this.loadRecords();
  }

  renderSummary(data) {
    this.summary.innerHTML = html`
      <article class="stat-card"><span>總記憶</span><strong>${data.total}</strong></article>
      <article class="stat-card"><span>Scopes</span><strong>${(data.by_scope || []).length}</strong></article>
      <article class="stat-card"><span>Consolidate 候選</span><strong>${data.consolidation_candidates}</strong></article>
      <article class="stat-card"><span>Prune 候選</span><strong>${data.prune_candidates}</strong></article>
      <article class="stat-card"><span>Embedding</span><strong>${data.embedding.embedded}/${data.embedding.total}</strong></article>
    `.toString();
  }

  renderScopes(scopes) {
    const current = this.scopeSelect.value;
    this.scopeSelect.innerHTML = '<option value="">全部</option>' + scopes.map((s) =>
      '<option value="' + encodeURIComponent(s.scope) + '">' + s.scope + ' (' + s.count + ')</option>'
    ).join('');
    this.scopeSelect.value = current;
  }

  async loadRecords() {
    const params = new URLSearchParams();
    if (window.WUKONG_TOKEN) params.set('token', window.WUKONG_TOKEN);
    if (this.scopeSelect.value) params.set('scope', decodeURIComponent(this.scopeSelect.value));
    if (this.kindSelect.value) params.set('kind', this.kindSelect.value);
    params.set('limit', '20');
    const resp = await fetch('/api/memory/records?' + params.toString());
    if (!resp.ok) {
      this.records.textContent = '無法讀取記憶列表：HTTP ' + resp.status;
      return;
    }
    const page = await resp.json();
    this.records.innerHTML = (page.records || []).map((record) => html`
      <article class="record-card">
        <div><span class="tag">${record.scope}</span> <span class="tag">${record.kind}</span></div>
        <p>${record.text}</p>
        <small>importance ${record.importance} · recalled ${record.recall_count} · ${new Date(record.created_at * 1000).toLocaleString('zh-TW')}</small>
      </article>
    `.toString()).join('') || '<p class="empty-state">沒有記憶。</p>';
  }

  async runRecall() {
    const query = this.recallQuery.value.trim();
    if (!query) {
      this.recallStatus.textContent = '請先輸入查詢內容。';
      this.recallResults.innerHTML = '';
      return;
    }
    const topK = Number.parseInt(this.recallTopK.value || '8', 10);
    this.recallStatus.textContent = '查詢中…';
    this.recallResults.innerHTML = '';
    const params = new URLSearchParams();
    if (window.WUKONG_TOKEN) params.set('token', window.WUKONG_TOKEN);
    const resp = await fetch('/api/memory/recall-preview' + (params.toString() ? '?' + params.toString() : ''), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        query,
        scope: this.scopeSelect.value ? decodeURIComponent(this.scopeSelect.value) : undefined,
        top_k: Number.isFinite(topK) ? topK : 8,
        mode: 'hybrid',
      }),
    });
    if (!resp.ok) {
      this.recallStatus.textContent = '查詢失敗：HTTP ' + resp.status + ' ' + await resp.text();
      return;
    }
    const data = await resp.json();
    this.renderRecallResults(data);
  }

  renderRecallResults(data) {
    const hits = data.hits || [];
    this.recallStatus.textContent = '命中 ' + hits.length + ' 筆 · confidence ' + data.confidence + ' · ' + data.latency_ms + 'ms';
    this.recallResults.innerHTML = hits.map((hit) => html`
      <article class="record-card">
        <div><span class="tag">${hit.scope}</span> <span class="tag">${hit.kind}</span> <span class="tag">score ${this.formatScore(hit.score)}</span></div>
        <p>${hit.text}</p>
        ${unsafe(this.recallExplanationHtml(hit.explanation))}
      </article>
    `.toString()).join('') || '<p class="empty-state">沒有符合的記憶。</p>';
  }

  formatScore(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number.toFixed(3) : '0.000';
  }

  recallExplanationHtml(explanation) {
    if (!explanation) return '';
    const signals = (explanation.source_signals || []).join(', ') || 'none';
    return html`
      <small>
        signals ${signals} · lexical ${this.formatScore(explanation.lexical)} · semantic ${this.formatScore(explanation.semantic)} · decay ${this.formatScore(explanation.decay)} · importance ${this.formatScore(explanation.importance)} · bonus ${this.formatScore(explanation.recall_bonus)} · age ${explanation.age_seconds}s · recalled ${explanation.recall_count}
      </small>
    `.toString();
  }
}
