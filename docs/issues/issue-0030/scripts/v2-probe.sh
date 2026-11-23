#!/bin/sh
# 步驟 4：OpenCode V2 最小契約探測。在跑著 `opencode serve`（V2）的隔離容器裡執行：
#   docker exec -e PW=... oc2-live sh /scripts/v2-probe.sh
# 每項印出 PASS／FAIL 與實際 HTTP 狀態；FAIL 不中斷，後面的項目照跑。
# 會送三個極短的 prompt 給預設的免費模型；不帶任何專案或使用者資料。
B=${B:-http://127.0.0.1:4096}
a() { curl -s -u "opencode:$PW" "$@"; }
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))" "$1"; }
result() { printf '%-4s %-28s %s\n' "$1" "$2" "$3"; }
wait_for() { # 等事件檔出現指定字串，最多 $2 秒
  i=0; while [ $i -lt "$2" ]; do grep -q "$1" "$3" && return 0; sleep 1; i=$((i + 1)); done; return 1
}

# 認證：沒帶密碼要 401
code=$(curl -s -o /dev/null -w '%{http_code}' "$B/api/session")
[ "$code" = 401 ] && result PASS auth.required "no-auth=$code" || result FAIL auth.required "no-auth=$code"

# V1 路徑：GET 應該失敗；回 200 代表 SPA 吞掉了路徑（健康檢查會誤判）
for p in /global/health "/session?limit=10" /event; do
  code=$(curl -s -o /tmp/v1body -w '%{http_code}' --max-time 3 -u "opencode:$PW" "$B$p")
  if [ "$code" = 200 ] && grep -qi '<!doctype html' /tmp/v1body; then
    result FAIL "v1.get$p" "http=$code body=html"
  else
    result PASS "v1.get$p" "http=$code"
  fi
done

# 權限：shell 設為 ask，模型要跑指令時應出現 permission.asked，回覆 once 後完成
EV=/tmp/ev-perm.txt; : > $EV
(curl -s -N -u "opencode:$PW" "$B/api/event" > $EV &)
sleep 1
SID=$(a -X POST -H 'content-type: application/json' \
  -d '{"title":"probe-permission","permissions":[{"action":"shell","resource":"*","effect":"ask"}]}' \
  "$B/api/session" | j 'd["data"]["id"]')
a -X POST -H 'content-type: application/json' \
  -d '{"text":"Use the shell tool to run exactly: echo probe. Then reply DONE."}' "$B/api/session/$SID/prompt" > /dev/null
if wait_for '"permission.asked"' 90 $EV; then
  RID=$(a "$B/api/session/$SID/permission" | j 'd["data"][0]["id"] if isinstance(d.get("data"), list) else d[0]["id"]')
  code=$(a -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' \
    -d '{"decision":"once"}' "$B/api/session/$SID/permission/$RID/reply")
  if wait_for '"session.execution.succeeded"' 90 $EV; then
    result PASS permission.ask_reply "reply=$code"
  else
    result FAIL permission.ask_reply "reply=$code no execution.succeeded"
  fi
else
  result FAIL permission.ask_reply "no permission.asked within 90s"
fi

# 取消：長回答中途 interrupt，應回 interrupted=true 並結束執行
EV=/tmp/ev-int.txt; : > $EV
(curl -s -N -u "opencode:$PW" "$B/api/event" > $EV &)
sleep 1
SID2=$(a -X POST -H 'content-type: application/json' -d '{"title":"probe-interrupt"}' "$B/api/session" | j 'd["data"]["id"]')
a -X POST -H 'content-type: application/json' \
  -d '{"text":"Count from 1 to 400, one number per line."}' "$B/api/session/$SID2/prompt" > /dev/null
wait_for '"session.text.delta"' 60 $EV
body=$(a -X POST "$B/api/session/$SID2/interrupt")
if echo "$body" | grep -q '"interrupted":true' && wait_for '"session.execution.interrupted"' 30 $EV; then
  result PASS interrupt "interrupted=true, session.execution.interrupted"
else
  result FAIL interrupt "$body"
fi

# 壓縮、換模型、讀訊息、刪除
code=$(a -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{}' "$B/api/session/$SID/compact")
[ "$code" = 200 ] && result PASS compact "http=$code" || result FAIL compact "http=$code"
code=$(a -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' \
  -d '{"model":{"id":"big-pickle","providerID":"opencode"}}' "$B/api/session/$SID/model")
[ "$code" = 204 ] && result PASS model.switch "http=$code" || result FAIL model.switch "http=$code"
n=$(a "$B/api/session/$SID/message?limit=100" | j 'len(d["data"])')
[ "$n" -gt 0 ] 2>/dev/null && result PASS message.list "messages=$n" || result FAIL message.list "messages=$n"
code=$(a -o /dev/null -w '%{http_code}' -X DELETE "$B/api/session/$SID2")
after=$(a -o /dev/null -w '%{http_code}' "$B/api/session/$SID2")
[ "$code" = 204 ] && [ "$after" = 404 ] && result PASS session.delete "delete=$code get-after=$after" \
  || result FAIL session.delete "delete=$code get-after=$after"

echo "--- event types seen"
cat /tmp/ev-perm.txt /tmp/ev-int.txt | grep -o '"type":"[a-z._]*"' | sort | uniq -c | sort -rn
