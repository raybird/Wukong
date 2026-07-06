import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const source = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), 'wukong-chat.js'),
  'utf8'
);

test('historical turn events open and load thinking without a manual toggle', () => {
  const method = source.match(/\n  lazyEventsNode\(message\) \{[\s\S]*?\n  \}\n\n  isNearBottom\(\)/)?.[0] || '';

  assert.match(method, /details\.open\s*=\s*true/);
  assert.match(method, /void\s+loadEvents\(\)/);
});
