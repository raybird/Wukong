// 進度列的角色文字（#45 SCN-002）。role 是派出、還沒回報的 subagent；空的表示悟空自己在處理
// （2026-11-27 起 subagent 都回報後，server 送空的 role）。回傳純文字，呼叫端負責跳脫。
export function roleProgressText(role) {
  return role ? `🐵 悟空·${role} 思考中…` : '🐵 思考中…';
}
