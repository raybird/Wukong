// Provider → Model 兩級選單（issue-0026）。Settings 與排程卡片共用。
// 清單來自 GET /api/models；取不到或已存的模型不在清單時改用手動輸入，保留原值。
export const MANUAL = '__manual__';

export function initialPickerState(catalog, current) {
  const providers = catalog?.providers ?? [];
  const value = (current || '').trim();
  if (!providers.length) {
    return { provider: MANUAL, model: '', manual: value, notListed: false, listAvailable: false };
  }
  if (!value) {
    return { provider: providers[0].id, model: '', manual: '', notListed: false, listAvailable: true };
  }
  const group = providers.find((p) => p.models.includes(value));
  if (group) {
    return { provider: group.id, model: value, manual: '', notListed: false, listAvailable: true };
  }
  return { provider: MANUAL, model: '', manual: value, notListed: true, listAvailable: true };
}

export function modelsFor(catalog, provider) {
  return catalog?.providers?.find((p) => p.id === provider)?.models ?? [];
}

export function pickedModel(state) {
  return (state.provider === MANUAL ? state.manual : state.model).trim();
}

/** 取得模型清單；失敗時回 null，由選單退回手動輸入。 */
export async function loadModelCatalog(tokenParam) {
  try {
    const resp = await fetch('/api/models' + tokenParam);
    return resp.ok ? await resp.json() : null;
  } catch {
    return null;
  }
}

function option(value, label, selected) {
  const el = document.createElement('option');
  el.value = value;
  el.textContent = label;
  el.selected = selected;
  return el;
}

/**
 * 在 container 內建立兩級選單，回傳 `{ value(), setDisabled(disabled) }`。
 * `value()` 回傳選定的完整模型 ID，沒選時回空字串。
 */
export function mountModelPicker(container, { catalog, current }) {
  const state = initialPickerState(catalog, current);
  container.classList.add('model-picker');
  const providerSelect = document.createElement('select');
  providerSelect.setAttribute('aria-label', 'Provider');
  const modelSelect = document.createElement('select');
  modelSelect.setAttribute('aria-label', 'Model');
  const manualInput = document.createElement('input');
  manualInput.type = 'text';
  manualInput.placeholder = 'provider/model';
  manualInput.setAttribute('aria-label', '自訂模型');
  manualInput.value = state.manual;
  const note = document.createElement('span');
  note.className = 'model-picker-note';

  for (const p of catalog?.providers ?? []) {
    providerSelect.append(option(p.id, p.id, p.id === state.provider));
  }
  providerSelect.append(option(MANUAL, '自訂（手動輸入）', state.provider === MANUAL));

  const render = () => {
    const manual = state.provider === MANUAL;
    modelSelect.replaceChildren(option('', '請選擇模型', !state.model));
    for (const id of modelsFor(catalog, state.provider)) {
      modelSelect.append(option(id, id.slice(state.provider.length + 1), id === state.model));
    }
    modelSelect.hidden = manual;
    manualInput.hidden = !manual;
    note.textContent = !state.listAvailable
      ? '無法取得模型清單，請手動輸入。'
      : state.notListed && manual
        ? '目前的模型不在清單中。'
        : '';
  };

  providerSelect.addEventListener('change', () => {
    state.provider = providerSelect.value;
    state.model = '';
    render();
  });
  modelSelect.addEventListener('change', () => {
    state.model = modelSelect.value;
  });
  manualInput.addEventListener('input', () => {
    state.manual = manualInput.value;
  });

  if (!state.listAvailable) providerSelect.hidden = true;
  container.replaceChildren(providerSelect, modelSelect, manualInput, note);
  render();
  return {
    value: () => pickedModel(state),
    setDisabled(disabled) {
      for (const el of [providerSelect, modelSelect, manualInput]) el.disabled = disabled;
    },
  };
}
