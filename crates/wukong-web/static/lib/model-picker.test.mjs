import test from 'node:test';
import assert from 'node:assert/strict';
import { MANUAL, initialPickerState, modelsFor, pickedModel } from './model-picker.mjs';

// 形狀同 GET /api/models；nvidia 的模型 ID 含多段斜線。
const catalog = {
  providers: [
    { id: 'opencode', models: ['opencode/a', 'opencode/b'] },
    { id: 'nvidia', models: ['nvidia/deepseek-ai/deepseek-v4.1-flash'] },
  ],
};

// SCN-002／SCN-003：已存的模型在清單裡，選單停在它所屬的 Provider。
test('a listed model selects its provider and itself', () => {
  const state = initialPickerState(catalog, 'nvidia/deepseek-ai/deepseek-v4.1-flash');
  assert.equal(state.provider, 'nvidia');
  assert.equal(state.model, 'nvidia/deepseek-ai/deepseek-v4.1-flash');
  assert.equal(state.notListed, false);
  assert.equal(pickedModel(state), 'nvidia/deepseek-ai/deepseek-v4.1-flash');
});

// SCN-002：切換 Provider 時，模型選項只列該 Provider 的模型。
test('models are listed per provider', () => {
  assert.deepEqual(modelsFor(catalog, 'opencode'), ['opencode/a', 'opencode/b']);
  assert.deepEqual(modelsFor(catalog, 'nvidia'), ['nvidia/deepseek-ai/deepseek-v4.1-flash']);
  assert.deepEqual(modelsFor(catalog, MANUAL), []);
  assert.deepEqual(modelsFor(null, 'opencode'), []);
});

// SCN-004：沒有設定模型時，預設停在第一個 Provider，尚未選模型。
test('no current model starts on the first provider with nothing picked', () => {
  const state = initialPickerState(catalog, '');
  assert.equal(state.provider, 'opencode');
  assert.equal(pickedModel(state), '');
});

// SCN-005：已存的模型不在清單裡，照原值顯示並標示，不被換成清單中的模型。
test('an unlisted current model is kept as manual input and flagged', () => {
  const state = initialPickerState(catalog, 'custom/model-x');
  assert.equal(state.provider, MANUAL);
  assert.equal(state.notListed, true);
  assert.equal(pickedModel(state), 'custom/model-x');
});

// SCN-005：清單取不到（API 失敗）或是空的，退回手動輸入，保留原值。
test('a missing or empty catalog falls back to manual input', () => {
  for (const missing of [null, { providers: [] }]) {
    const state = initialPickerState(missing, 'opencode/a');
    assert.equal(state.provider, MANUAL);
    assert.equal(state.listAvailable, false);
    assert.equal(state.notListed, false);
    assert.equal(pickedModel(state), 'opencode/a');
  }
});

test('the picked model is trimmed', () => {
  assert.equal(pickedModel({ provider: MANUAL, model: '', manual: '  opencode/a  ' }), 'opencode/a');
  assert.equal(pickedModel({ provider: 'opencode', model: 'opencode/b', manual: 'x' }), 'opencode/b');
});
