import test from 'node:test';
import assert from 'node:assert/strict';
import {
  firstUnreadIndex,
  lastSeenKey,
  latestMessageId,
  readLastSeenMessageId,
  writeLastSeenMessageId,
} from './unread-marker.mjs';

class MemoryStorage {
  constructor(entries = {}) {
    this.map = new Map(Object.entries(entries));
  }

  getItem(key) {
    return this.map.has(key) ? this.map.get(key) : null;
  }

  setItem(key, value) {
    this.map.set(key, String(value));
  }
}

test('lastSeenKey is scoped', () => {
  assert.equal(lastSeenKey('global'), 'wukong.chat.lastSeenMessageId:global');
  assert.equal(lastSeenKey('user:tg-915354960'), 'wukong.chat.lastSeenMessageId:user:tg-915354960');
});

test('readLastSeenMessageId ignores missing invalid and non-positive values', () => {
  assert.equal(readLastSeenMessageId(new MemoryStorage(), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: 'abc' }), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: '0' }), 'global'), null);
  assert.equal(readLastSeenMessageId(new MemoryStorage({ [lastSeenKey('global')]: '-7' }), 'global'), null);
});

test('readLastSeenMessageId reads positive integer values', () => {
  const storage = new MemoryStorage({ [lastSeenKey('global')]: '42' });
  assert.equal(readLastSeenMessageId(storage, 'global'), 42);
});

test('writeLastSeenMessageId writes only positive latest ids', () => {
  const storage = new MemoryStorage();
  assert.equal(writeLastSeenMessageId(storage, 'global', null), false);
  assert.equal(writeLastSeenMessageId(storage, 'global', 0), false);
  assert.equal(writeLastSeenMessageId(storage, 'global', 7), true);
  assert.equal(storage.getItem(lastSeenKey('global')), '7');
});

test('latestMessageId returns largest numeric message id', () => {
  assert.equal(latestMessageId([{ id: 3 }, { id: '9' }, { id: 'bad' }, { id: 5 }]), 9);
  assert.equal(latestMessageId([{ id: 'bad' }]), null);
  assert.equal(latestMessageId([]), null);
});

test('firstUnreadIndex returns -1 when no stored marker exists', () => {
  assert.equal(firstUnreadIndex([{ id: 1 }, { id: 2 }], null), -1);
});

test('firstUnreadIndex finds first message newer than marker', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }, { id: 12 }], 10), 1);
});

test('firstUnreadIndex returns first message when whole latest page is new', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 3), 0);
});

test('firstUnreadIndex returns -1 when marker is current or newer', () => {
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 11), -1);
  assert.equal(firstUnreadIndex([{ id: 10 }, { id: 11 }], 99), -1);
});
