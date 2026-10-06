#!/usr/bin/env python3
"""Merge one run's P3 results into parity/results/p3/<run>.json (counts only).

Usage: merge_p3.py <run> <predictions summary.json> <score summary.json> <bench diff.out> <out.json>

The bench diff's first line (its tally) is kept; its per-row lines are not.
"""
import json
import re
import sys

run, preds, score, diff_out, out = sys.argv[1:6]
with open(diff_out, encoding="utf-8") as f:
    tally = f.readline().strip()
m = re.match(r"(\d+) worlds compared, (\d+) rows in a, (\d+) in b: (\d+) differences \((\d+) only in a, (\d+) only in b, (\d+) changed, (\d+) order\)", tally)
bench_diff = dict(zip(["worlds", "rows_a", "rows_b", "differences", "only_a", "only_b", "changed", "order"], map(int, m.groups()))) if m else {"tally": tally}
with open(preds, encoding="utf-8") as f:
    p = json.load(f)
with open(score, encoding="utf-8") as f:
    s = json.load(f)
result = "equal" if p["result"] == "equal" and s["result"] == "equal" else "differs"
json.dump(
    {
        "run": run,
        "result": result,
        "bench_diff_normalize_ids": bench_diff,
        "predictions_modulo_agent_ids": p,
        "score_vs_p2": {k: s[k] for k in ("result", "compared_values", "differences")},
        "score": s.get("a2a_reference"),
    },
    open(out, "w", encoding="utf-8"),
    indent=2,
)
print(run, result)
