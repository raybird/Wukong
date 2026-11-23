"""在 RunWuKong 的唯讀掛載上，用 SQLite backup API 取一致性副本（含 WAL 內容）。

執行方式見 ../inventory.md「步驟 1」。來源一律以 mode=ro 開啟，掛載也是 :ro。
"""
import hashlib
import os
import sqlite3

PAIRS = [
    ("/src/data/memory.db", "/dst/memory.db"),
    ("/src/opencode/opencode.db", "/dst/opencode.db"),
    ("/src/memoria/.memory/sessions.db", "/dst/memoria-sessions.db"),
    ("/src/memoria/.memory/vectors.db", "/dst/memoria-vectors.db"),
]

for src, dst in PAIRS:
    if os.path.exists(dst):
        os.remove(dst)
    source = sqlite3.connect(f"file:{src}?mode=ro", uri=True)
    target = sqlite3.connect(dst)
    source.backup(target)
    source.close()
    try:
        integrity = target.execute("pragma integrity_check").fetchone()[0]
    except sqlite3.OperationalError as err:  # vectors.db 的索引要 libsql 才驗得了
        integrity = f"unknown ({err})"
    tables = target.execute("select count(*) from sqlite_master where type='table'").fetchone()[0]
    target.close()
    digest = hashlib.sha256(open(dst, "rb").read()).hexdigest()
    print(f"{src} bytes={os.path.getsize(dst)} tables={tables} integrity={integrity} sha256={digest}")
