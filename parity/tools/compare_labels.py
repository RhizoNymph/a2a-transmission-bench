#!/usr/bin/env python3
"""Compare two exports' labels.jsonl modulo label ids.

Usage: compare_labels.py <a export dir> <b export dir> --run NAME
                         [--a-label L] [--b-label L] [--out summary.json]

`a2a-bench diff` keys label rows by id, and several converters name their
labels differently from ct-eval's golden export by design (the converter
docs list them: SALT `t<n>` equal, wiki paths, swarm-traces source paths,
cipher `cipher/<key>`, ...). This tool compares each world's label rows as
a multiset with `id` removed, so only a difference in content, location,
route, needs, tier, reason or source remains. For rows left unmatched it
pairs them by (kind, from, to, reader exchange, location) and counts the
top-level fields that differ, by field name; it never prints a value.

The world rows (key, coverage) and `exchange_agent` rows are compared too.
Files are streamed one world at a time. Exit 0 when equal, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from typing import Any, Iterator


def worlds(path: str) -> Iterator[tuple[str, str, list[dict[str, Any]]]]:
    current: dict[str, Any] | None = None
    body: list[dict[str, Any]] = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            r = json.loads(line)
            kind = r["kind"]
            if kind == "header":
                continue
            if kind == "world":
                if current is not None:
                    yield current["key"], json.dumps(current, sort_keys=True), body
                current, body = r, []
            elif kind == "trailer":
                break
            else:
                body.append(r)
    if current is not None:
        yield current["key"], json.dumps(current, sort_keys=True), body


def canon(row: dict[str, Any]) -> str:
    return json.dumps({k: v for k, v in row.items() if k != "id"}, sort_keys=True)


def pair_key(row: dict[str, Any]) -> str:
    at = row.get("at") or (row.get("content") or {}).get("at")
    return json.dumps(
        [row.get("kind"), row.get("from"), row.get("to"), row.get("reader_exchange"), at],
        sort_keys=True,
    )


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--run", required=True)
    p.add_argument("--a-label", default="golden")
    p.add_argument("--b-label", default="bench")
    p.add_argument("--out")
    args = p.parse_args()

    rows_a: Counter[str] = Counter()
    rows_b: Counter[str] = Counter()
    matched: Counter[str] = Counter()
    only_a: Counter[str] = Counter()
    only_b: Counter[str] = Counter()
    fields_differ: Counter[str] = Counter()
    unpaired_a: Counter[str] = Counter()
    unpaired_b: Counter[str] = Counter()
    ids_equal = 0
    ids_differ = 0
    worlds_n = 0
    worlds_equal = 0
    world_rows_differ = 0
    keys_differ = 0
    first_worlds: list[str] = []

    ia = worlds(f"{args.a}/labels.jsonl")
    ib = worlds(f"{args.b}/labels.jsonl")
    for (ka, wa, ra), (kb, wb, rb) in zip(ia, ib, strict=True):
        worlds_n += 1
        if ka != kb:
            keys_differ += 1
            continue
        if wa != wb:
            world_rows_differ += 1
        ca = Counter(canon(r) for r in ra)
        cb = Counter(canon(r) for r in rb)
        for r in ra:
            rows_a[r["kind"]] += 1
        for r in rb:
            rows_b[r["kind"]] += 1
        both = ca & cb
        for c, n in both.items():
            matched[json.loads(c)["kind"]] += n
        # Ids of rows equal apart from the id, in order.
        ida = [(canon(r), r.get("id")) for r in ra if "id" in r]
        idb = dict((canon(r), r.get("id")) for r in rb if "id" in r)
        for c, i in ida:
            if c in idb:
                if idb[c] == i:
                    ids_equal += 1
                else:
                    ids_differ += 1
        oa = ca - cb
        ob = cb - ca
        if not oa and not ob and wa == wb:
            worlds_equal += 1
        elif len(first_worlds) < 20:
            first_worlds.append(ka)
        rest_a: dict[str, list[dict[str, Any]]] = {}
        for c, n in oa.items():
            row = json.loads(c)
            only_a[row["kind"]] += n
            rest_a.setdefault(pair_key(row), []).extend([row] * n)
        for c, n in ob.items():
            row = json.loads(c)
            only_b[row["kind"]] += n
            for _ in range(n):
                cands = rest_a.get(pair_key(row))
                if cands:
                    other = cands.pop()
                    for k in sorted(set(other) | set(row)):
                        if other.get(k) != row.get(k):
                            fields_differ[f"{row['kind']}.{k}"] += 1
                else:
                    unpaired_b[row["kind"]] += 1
        for rows in rest_a.values():
            for row in rows:
                unpaired_a[row["kind"]] += 1

    equal = not only_a and not only_b and keys_differ == 0 and world_rows_differ == 0
    summary = {
        "run": args.run,
        "file": "labels.jsonl",
        "a": args.a_label,
        "b": args.b_label,
        "result": "equal modulo ids" if equal else "differs",
        "worlds": worlds_n,
        "worlds_equal": worlds_equal,
        "world_keys_differ": keys_differ,
        "world_rows_differ": world_rows_differ,
        "rows": {args.a_label: dict(rows_a), args.b_label: dict(rows_b)},
        "matched_without_id": dict(matched),
        "ids_of_matched_rows": {"equal": ids_equal, "differ": ids_differ},
        f"only_{args.a_label}": dict(only_a),
        f"only_{args.b_label}": dict(only_b),
        "paired_fields_differ": dict(fields_differ),
        f"unpaired_{args.a_label}": dict(unpaired_a),
        f"unpaired_{args.b_label}": dict(unpaired_b),
        "first_differing_worlds": first_worlds,
    }
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(json.dumps(summary, indent=2) + "\n")
    print(
        f"{args.run}: {summary['result']} worlds={worlds_n} equal={worlds_equal} "
        f"matched={dict(matched)} ids equal/differ={ids_equal}/{ids_differ} "
        f"only_a={dict(only_a)} only_b={dict(only_b)} fields={dict(fields_differ)}"
    )
    return 0 if equal else 1


if __name__ == "__main__":
    sys.exit(main())
