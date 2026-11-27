"""#45 Task 6.1（SCN-003）派工門檻的判定。

    python3 -I dispatch_judge.py RESULTS.jsonl MODEL
    python3 -I dispatch_judge.py --self-check

RESULTS.jsonl 每行一次回合：{"task", "run", "subagents": [agent...], "models": [provider/model...], "ok": bool}
（dispatch-eval.sh 產生）。門檻：
- implement：每次都派給 fixer；direct：每次都沒有派工；
- 其他四題：至少 2/3 的回合派給預期的 subagent；
- 每次回合都要成功，primary 與 subagent 都用 MODEL，否則那次不算數並判失敗。
--self-check 以已知結果的紀錄確認判準判得出「沒派工」「派錯」「直接回答卻派工」「用錯模型」，也判得過正確的一組。
"""
import json
import sys

EXPECTED = {
    "implement": "fixer",
    "explore": "explorer",
    "lookup": "librarian",
    "architecture": "oracle",
    "design": "designer",
    "direct": None,
}


def judge(records, model):
    lines, ok = [], True
    for task, want in EXPECTED.items():
        runs = [r for r in records if r["task"] == task]
        invalid = [r["run"] for r in runs if not r["ok"] or set(r["models"]) != {model}]
        if want is None:
            hits = sum(1 for r in runs if not r["subagents"])
            need = len(runs)
        else:
            hits = sum(1 for r in runs if want in r["subagents"])
            need = len(runs) if task == "implement" else 2
        passed = len(runs) == 3 and not invalid and hits >= need
        ok &= passed
        got = "; ".join(f"run {r['run']}: {','.join(r['subagents']) or '不派工'}" for r in runs)
        lines.append(f"{'PASS' if passed else 'FAIL'} {task}: 預期 {want or '不派工'}，{hits}/{len(runs)}（需 {need}）"
                     f"{'，無效回合 ' + str(invalid) if invalid else ''} — {got}")
    return ok, lines


def self_check():
    model = "p/m"
    def rec(task, run, subagents, models=(model,), ok=True):
        return {"task": task, "run": run, "subagents": list(subagents), "models": list(models), "ok": ok}
    good = []
    for task, want in EXPECTED.items():
        for run in (1, 2, 3):
            good.append(rec(task, run, [want] if want else []))
    cases = {
        "全部正確": (good, True),
        "實作題有一次沒派工": ([r if (r["task"], r["run"]) != ("implement", 2) else rec("implement", 2, []) for r in good], False),
        "探索題兩次派錯": ([r if not (r["task"] == "explore" and r["run"] < 3) else rec("explore", r["run"], ["fixer"]) for r in good], False),
        "探索題一次派錯（仍達 2/3）": ([r if (r["task"], r["run"]) != ("explore", 1) else rec("explore", 1, ["fixer"]) for r in good], True),
        "直接回答題派了工": ([r if (r["task"], r["run"]) != ("direct", 3) else rec("direct", 3, ["explorer"]) for r in good], False),
        "subagent 用了別的模型": ([r if (r["task"], r["run"]) != ("design", 1) else rec("design", 1, ["designer"], (model, "x/y")) for r in good], False),
        "回合失敗": ([r if (r["task"], r["run"]) != ("lookup", 1) else rec("lookup", 1, ["librarian"], ok=False) for r in good], False),
        "少一次回合": ([r for r in good if (r["task"], r["run"]) != ("architecture", 3)], False),
    }
    bad = 0
    for name, (records, want) in cases.items():
        got, _ = judge(records, model)
        print(f"{'PASS' if got == want else 'FAIL'} self-check {name}: 判定 {got}，預期 {want}")
        bad += got != want
    return bad == 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-check"]:
        sys.exit(0 if self_check() else 1)
    records = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
    ok, lines = judge(records, sys.argv[2])
    print("\n".join(lines))
    sys.exit(0 if ok else 1)
