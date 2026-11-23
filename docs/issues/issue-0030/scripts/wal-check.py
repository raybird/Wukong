"""反向自檢：副本的總筆數要等於「主檔＋WAL」原始複本，且大於「只有主檔」。

只有主檔時筆數較少，才證明副本確實含 WAL 內容；兩者相等時這項檢查不成立。
"""
import os
import shutil
import sqlite3

SETS = [
    ("/src/data", "memory.db", "/dst/memory.db"),
    ("/src/opencode", "opencode.db", "/dst/opencode.db"),
]


def total_rows(uri):
    conn = sqlite3.connect(uri, uri=True)
    names = conn.execute(
        "select name from sqlite_master where type='table' and name not like 'sqlite_%'"
    ).fetchall()
    total = sum(conn.execute(f'select count(*) from "{n}"').fetchone()[0] for (n,) in names)
    conn.close()
    return total


for src_dir, name, snapshot in SETS:
    with_wal = f"/work/wal/{name}"
    main_only = f"/work/main/{name}"
    os.makedirs(os.path.dirname(with_wal), exist_ok=True)
    os.makedirs(os.path.dirname(main_only), exist_ok=True)
    shutil.copy2(f"{src_dir}/{name}", with_wal)
    shutil.copy2(f"{src_dir}/{name}-wal", f"{with_wal}-wal")
    shutil.copy2(f"{src_dir}/{name}", main_only)
    snap, full, main = (total_rows(f"file:{snapshot}?immutable=1"), total_rows(f"file:{with_wal}"),
                        total_rows(f"file:{main_only}?immutable=1"))
    verdict = "PASS" if snap == full and full > main else "FAIL"
    print(name, verdict, "snapshot", snap, "main+wal", full, "main-only", main)
