"""步驟 2：資料庫聚合盤點。只輸出數字，不輸出任何欄位內容。

用法（在一次性容器裡）：
  python3 -I inventory.py control          # 對照組：造已知筆數的資料庫，取數結果要完全相符
  python3 -I inventory.py /dst/opencode.db /dst/memory.db ...

查不到的表或欄位記 "unknown"，不當成 0。
"""
import json
import os
import sqlite3
import sys
import tempfile

UNKNOWN = "unknown"


def connect(path):
    return sqlite3.connect(f"file:{path}?immutable=1", uri=True)


def scalar(conn, sql, args=()):
    try:
        return conn.execute(sql, args).fetchone()[0]
    except sqlite3.OperationalError:
        return UNKNOWN


def tables(conn):
    return [n for (n,) in conn.execute(
        "select name from sqlite_master where type='table' and name not like 'sqlite_%' order by name")]


def table_bytes(conn):
    """每張表（含它的索引）占用的 byte 數，來自 dbstat。"""
    try:
        rows = conn.execute(
            "select coalesce(m.tbl_name, d.name), sum(d.pgsize) from dbstat d "
            "left join sqlite_master m on m.name = d.name group by 1").fetchall()
    except sqlite3.OperationalError:
        return None
    return dict(rows)


def top_sessions(conn, table, n=5):
    """依 session_id 分組，取筆數最多的前 n 個 session 的筆數與 data 總長度。"""
    sql = (f'select count(*), sum(length(data)) from "{table}" group by session_id '
           f'order by count(*) desc limit ?')
    try:
        return [list(r) for r in conn.execute(sql, (n,))]
    except sqlite3.OperationalError:
        return UNKNOWN


def part_types(conn):
    try:
        rows = conn.execute(
            "select json_extract(data, '$.type'), count(*), sum(length(data)) from part group by 1 order by 3 desc")
        return {str(t): [c, b] for t, c, b in rows}
    except sqlite3.OperationalError:
        return UNKNOWN


def event_types(conn):
    try:
        rows = conn.execute("select type, count(*), sum(length(data)) from event group by 1 order by 3 desc")
        return {str(t): [c, b] for t, c, b in rows}
    except sqlite3.OperationalError:
        return UNKNOWN


def inventory(path):
    conn = connect(path)
    names = tables(conn)
    sizes = table_bytes(conn)
    report = {
        "file": os.path.basename(path),
        "bytes": os.path.getsize(path),
        "page_size": scalar(conn, "pragma page_size"),
        "page_count": scalar(conn, "pragma page_count"),
        "freelist_count": scalar(conn, "pragma freelist_count"),
        "tables": {
            n: {"rows": scalar(conn, f'select count(*) from "{n}"'),
                "bytes": UNKNOWN if sizes is None else sizes.get(n, UNKNOWN)}
            for n in names
        },
    }
    if "session" in names and "part" in names:  # opencode.db
        report["opencode"] = {
            "sessions": scalar(conn, "select count(*) from session"),
            "child_sessions": scalar(conn, "select count(*) from session where parent_id is not null"),
            "top_sessions_by_parts": top_sessions(conn, "part"),
            "top_sessions_by_messages": top_sessions(conn, "message"),
            "part_types": part_types(conn),
            "event_types": event_types(conn),
            "events_outside_sessions": scalar(
                conn, "select count(*) from event where aggregate_id not in (select id from session)"),
            "oldest_session_ms": scalar(conn, "select min(time_created) from session"),
            "newest_session_ms": scalar(conn, "select max(time_created) from session"),
        }
    conn.close()
    return report


def control():
    """造一個已知內容的資料庫：3 個 session，part 分別 7、3、0 筆；message 2、1、0 筆；event 3 筆，其中 1 筆不屬於任何 session。"""
    work = tempfile.mkdtemp()
    path = os.path.join(work, "control.db")
    conn = sqlite3.connect(path)
    conn.executescript(
        "create table session (id text primary key, parent_id text, time_created integer);"
        "create table message (id text primary key, session_id text, data text);"
        "create table part (id text primary key, session_id text, data text);"
        "create table event (id text primary key, aggregate_id text, type text, data text);")
    conn.executemany("insert into event values (?,?,?,?)",
                     [("e1", "a", "x", "12"), ("e2", "b", "x", "3"), ("e3", "gone", "y", "")])
    conn.executemany("insert into session values (?,?,?)",
                     [("a", None, 100), ("b", "a", 200), ("c", None, 300)])
    parts = [("a", "tool")] * 5 + [("a", "text")] * 2 + [("b", "text")] * 3
    conn.executemany("insert into part values (?,?,?)",
                     [(f"p{i}", s, json.dumps({"type": t})) for i, (s, t) in enumerate(parts)])
    conn.executemany("insert into message values (?,?,?)",
                     [("m1", "a", "{}"), ("m2", "a", "{}"), ("m3", "b", "{}")])
    conn.commit()
    conn.close()
    got = inventory(path)
    tool_len = len(json.dumps({"type": "tool"}))
    text_len = len(json.dumps({"type": "text"}))
    expected = {
        ("tables", "part", "rows"): 10,
        ("tables", "session", "rows"): 3,
        ("opencode", "sessions"): 3,
        ("opencode", "child_sessions"): 1,
        ("opencode", "top_sessions_by_parts"): [[7, 5 * tool_len + 2 * text_len], [3, 3 * text_len]],
        ("opencode", "top_sessions_by_messages"): [[2, 4], [1, 2]],
        ("opencode", "part_types"): {"tool": [5, 5 * tool_len], "text": [5, 5 * text_len]},
        ("opencode", "oldest_session_ms"): 100,
        ("opencode", "event_types"): {"x": [2, 3], "y": [1, 0]},
        ("opencode", "events_outside_sessions"): 1,
    }
    failures = []
    for keys, want in expected.items():
        value = got
        for k in keys:
            value = value[k]
        if value != want:
            failures.append(f"{'.'.join(keys)}: got {value!r}, want {want!r}")
    # 查不到的東西要是 unknown，不能是 0
    conn = connect(path)
    missing = scalar(conn, "select count(*) from no_such_table")
    conn.close()
    if missing != UNKNOWN:
        failures.append(f"missing table: got {missing!r}, want {UNKNOWN!r}")
    print(json.dumps({"control": "FAIL" if failures else "PASS", "failures": failures}, ensure_ascii=False))
    return 1 if failures else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["control"]:
        sys.exit(control())
    for p in sys.argv[1:]:
        print(json.dumps(inventory(p), ensure_ascii=False))
