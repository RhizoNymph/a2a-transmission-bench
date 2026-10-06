#!/usr/bin/env python3
"""Merge one run's P4 results into parity/results/p4/<run>.json (counts and ids only).

Usage: merge_p4.py <run> <golden export> <bench export> <bench diff.out> <labels summary.json> <out.json>

Records the bench diff's tally and its changed manifest paths, the
manifests' file digests and per-world entries compared, the manifest
fields that differ (source, converter, selection: settings and revisions,
no dataset text) and compare_labels.py's summary. Fields the parity plan
ignores (converter, source.digest, source.revision, source.path) are listed
under `ignored`.
"""
import json
import re
import sys

run, ga, ba, diff_out, labels, out = sys.argv[1:7]
IGNORED = ("/converter/", "/source/digest", "/source/revision", "/source/path")
with open(diff_out, encoding="utf-8") as f:
    lines = f.read().splitlines()
m = re.match(r"(\d+) worlds compared, (\d+) rows in a, (\d+) in b: (\d+) differences \((\d+) only in a, (\d+) only in b, (\d+) changed, (\d+) order\)", lines[0])
tally = dict(zip(["file_worlds", "rows_a", "rows_b", "differences", "only_a", "only_b", "changed", "order"], map(int, m.groups())))
manifest_paths = [l.split()[-1] for l in lines[1:] if l.strip().startswith("changed") and "manifest.json" in l]
a = json.load(open(f"{ga}/manifest.json", encoding="utf-8"))
b = json.load(open(f"{ba}/manifest.json", encoding="utf-8"))
lab = json.load(open(labels, encoding="utf-8"))
files_equal = {k: a["files"][k] == b["files"].get(k) for k in a["files"]}
considered = [p for p in manifest_paths if not p.startswith(IGNORED)]
messages_exchanges_equal = files_equal["messages"] and files_equal["exchanges"]
rows_equal = tally["only_a"] == 0 and tally["only_b"] == 0 and tally["order"] == 0
labels_ok = lab["result"] == "equal modulo ids"
if rows_equal and messages_exchanges_equal and files_equal.get("labels") and all(p == "/selection/split_list" for p in considered):
    result = "equal (byte-identical files; manifest differs only in ignored fields and split_list)"
elif messages_exchanges_equal and labels_ok:
    result = "equal modulo label ids"
else:
    result = "differs"
json.dump(
    {
        "run": run,
        "result": result,
        "bench_diff": tally,
        "manifest_paths_changed": manifest_paths,
        "ignored": [p for p in manifest_paths if p.startswith(IGNORED)],
        "considered": considered,
        "file_digests_equal": files_equal,
        "manifest_worlds_equal": a["worlds"] == b["worlds"],
        "worlds": len(a["worlds"]),
        "golden": {"source_revision": a["source"]["revision"], "selection": a["selection"], "pace": a.get("pace")},
        "bench": {"source_revision": b["source"]["revision"], "selection": b["selection"], "pace": b.get("pace")},
        "labels_modulo_ids": {k: v for k, v in lab.items() if k not in ("run", "file")},
    },
    open(out, "w", encoding="utf-8"),
    indent=2,
)
print(run, result, considered)
