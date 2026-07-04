const KEY_PREFIX = 'wukong.chat.lastSeenMessageId:';

export function lastSeenKey(scope) {
  return KEY_PREFIX + String(scope || 'global');
}

function parsePositiveInteger(value) {
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

export function readLastSeenMessageId(storage, scope) {
  try {
    return parsePositiveInteger(storage.getItem(lastSeenKey(scope)));
  } catch (_err) {
    return null;
  }
}

export function writeLastSeenMessageId(storage, scope, messageId) {
  const parsed = parsePositiveInteger(messageId);
  if (parsed === null) return false;
  try {
    storage.setItem(lastSeenKey(scope), String(parsed));
    return true;
  } catch (_err) {
    return false;
  }
}

export function latestMessageId(messages) {
  let latest = null;
  for (const message of messages || []) {
    const id = parsePositiveInteger(message && message.id);
    if (id !== null && (latest === null || id > latest)) latest = id;
  }
  return latest;
}

export function firstUnreadIndex(messages, marker) {
  const parsedMarker = parsePositiveInteger(marker);
  if (parsedMarker === null) return -1;
  const list = Array.isArray(messages) ? messages : [];
  for (let index = 0; index < list.length; index += 1) {
    const id = parsePositiveInteger(list[index] && list[index].id);
    if (id !== null && id > parsedMarker) return index;
  }
  return -1;
}
