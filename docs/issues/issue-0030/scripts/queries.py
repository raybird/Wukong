"""步驟 3：核心查詢的 EXPLAIN QUERY PLAN 與耗時。只輸出計畫、筆數與毫秒，不輸出內容。

用法：python3 -I queries.py /dst/memory.db /dst/opencode.db
冷：每次開新連線後第一次執行（作業系統的檔案快取仍可能是熱的）；暖：同一連線重複 20 次取中位數。
"""
import json
import sqlite3
import statistics
import sys
import time

WARM_RUNS = 20


def connect(path):
    return sqlite3.connect(f"file:{path}?immutable=1", uri=True)


def pick(conn, sql):
    row = conn.execute(sql).fetchone()
    return row[0] if row else None


def measure(path, name, sql, args):
    conn = connect(path)
    plan = [r[3] for r in conn.execute("explain query plan " + sql, args)]
    conn.close()
    conn = connect(path)
    start = time.perf_counter()
    rows = len(conn.execute(sql, args).fetchall())
    cold_ms = (time.perf_counter() - start) * 1000
    warm = []
    for _ in range(WARM_RUNS):
        start = time.perf_counter()
        conn.execute(sql, args).fetchall()
        warm.append((time.perf_counter() - start) * 1000)
    conn.close()
    return {"query": name, "rows": rows, "cold_ms": round(cold_ms, 3),
            "warm_median_ms": round(statistics.median(warm), 3), "plan": plan}


def memory_queries(path):
    conn = connect(path)
    scope = pick(conn, "select scope from agent_session_state order by updated_at desc limit 1")
    thread = pick(conn, "select thread_id from chat_messages group by thread_id order by count(*) desc limit 1")
    conn.close()
    return [
        ("recall.keyword_candidates",
         "SELECT m.id, m.scope, m.kind, m.text, m.created_at, m.recall_count, m.importance, bm25(memories_fts) AS bm25 "
         "FROM memories_fts JOIN memories m ON m.id = memories_fts.rowid WHERE memories_fts MATCH ? "
         "ORDER BY bm25 ASC LIMIT ?", ('"wukong" OR "opencode"', 20)),
        ("recall.cjk_fallback_candidates",
         "SELECT id FROM memories WHERE text LIKE ? ORDER BY created_at DESC LIMIT ?", ("%排程%", 20)),
        ("recall.recent_candidates",
         "SELECT id FROM memories ORDER BY created_at DESC LIMIT ?", (20,)),
        ("session.state_by_scope",
         "SELECT session_id, turn_count, lease_owner, lease_until FROM agent_session_state WHERE scope = ?", (scope,)),
        ("chat.messages_by_thread",
         "SELECT id FROM chat_messages WHERE thread_id = ? ORDER BY created_at, id", (thread,)),
    ]


def opencode_queries(path):
    conn = connect(path)
    biggest = pick(conn, "select session_id from part group by session_id order by count(*) desc limit 1")
    conn.close()
    return [
        ("opencode.session_list",
         "SELECT id FROM session ORDER BY time_updated DESC LIMIT 100", ()),
        ("opencode.messages_of_session",
         "SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created, id", (biggest,)),
        ("opencode.parts_of_session",
         "SELECT id, data FROM part WHERE session_id = ?", (biggest,)),
        ("opencode.parts_by_message",
         "SELECT p.id, p.data FROM message m JOIN part p ON p.message_id = m.id "
         "WHERE m.session_id = ? ORDER BY p.message_id, p.id", (biggest,)),
        ("opencode.events_of_session",
         "SELECT seq, type, data FROM event WHERE aggregate_id = ? ORDER BY seq", (biggest,)),
        ("baseline.full_scan_for_contrast",
         "SELECT count(*) FROM part WHERE json_extract(data, '$.type') = 'tool'", ()),
    ]


def recall_latency(path):
    conn = connect(path)
    rows = conn.execute("select mode, skipped, latency_ms from recall_telemetry").fetchall()
    conn.close()
    out = {}
    for mode, skipped, latency in rows:
        out.setdefault(f"{mode}/skipped={skipped}", []).append(latency)
    summary = {}
    for key, values in sorted(out.items()):
        values.sort()
        summary[key] = {"n": len(values), "p50": statistics.median(values),
                        "p95": values[min(len(values) - 1, int(len(values) * 0.95))], "max": values[-1]}
    return summary


if __name__ == "__main__":
    memory_db, opencode_db = sys.argv[1], sys.argv[2]
    for name, sql, args in memory_queries(memory_db):
        print(json.dumps({"db": "memory.db", **measure(memory_db, name, sql, args)}, ensure_ascii=False))
    for name, sql, args in opencode_queries(opencode_db):
        print(json.dumps({"db": "opencode.db", **measure(opencode_db, name, sql, args)}, ensure_ascii=False))
    print(json.dumps({"recall_telemetry_latency_ms": recall_latency(memory_db)}, ensure_ascii=False))
