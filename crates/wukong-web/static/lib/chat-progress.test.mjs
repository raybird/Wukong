import test from 'node:test';
import assert from 'node:assert/strict';
import { roleProgressText } from './chat-progress.mjs';

test('a dispatched subagent shows after 悟空', () => {
  assert.equal(roleProgressText('fixer'), '🐵 悟空·fixer 思考中…');
});

// review-623c436 NICE TO HAVE 1（2026-11-27）：subagent 都回報後，server 送空的 role，回到悟空自己處理。
test('an empty role means 悟空 is working again', () => {
  assert.equal(roleProgressText(''), '🐵 思考中…');
  assert.equal(roleProgressText(undefined), '🐵 思考中…');
});
