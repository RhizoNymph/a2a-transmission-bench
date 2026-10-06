#!/usr/bin/env python3
"""Compare two predictions files modulo detector agent ids and transmission ids.

Usage: compare_predictions.py <a/predictions.jsonl> <b/predictions.jsonl> --run NAME
                              [--a-label L] [--b-label L] [--out summary.json] [--first N]

`a2a-bench diff --normalize-ids` drops transmission ids but still keys
attribution rows by the detector's agent id and compares the agent ids
named inside transmissions. Two detectors that agree on everything but
how they name their agents (ct-eval's reference: one ULID per true agent;
`a2a-reference`: one `k:<digest>` per credential) therefore differ on every
row. This tool removes that naming:

- per world, each detector agent becomes the sorted tuple of the exchanges
  its `attribution` row holds; the partitions of the world's exchanges are
  compared (equal, or which exchange sets are only in one side);
- `unattributed` agents become `unattributed:<n>` in row order;
- transmissions drop `id`, have every agent id replaced by its canonical
  name, and are compared as a multiset of canonical JSON per world;
- `world` rows (key, status) are compared as JSON.

Files are streamed: one world's rows are held at a time. Only counts, world
keys, row kinds and digests of canonical rows are written; no row content.
Exit 0 when equal, 1 when different.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from dataclasses import dataclass, field
from typing import Any, Iterator


@dataclass
class World:
    key: str
    status: str
    agents: dict[str, str]
    partition: set[str]
    transmissions: Counter[str]
    unattributed: int


@dataclass
class Tally:
    worlds: int = 0
    worlds_equal: int = 0
    status_differs: int = 0
    partition_differs: int = 0
    transmissions_a: int = 0
    transmissions_b: int = 0
    transmissions_matched: int = 0
    transmissions_only_a: int = 0
    transmissions_only_b: int = 0
    attribution_rows_a: int = 0
    attribution_rows_b: int = 0
    unattributed_a: int = 0
    unattributed_b: int = 0
    world_order_differs: int = 0
    by_state_only_a: Counter[str] = field(default_factory=Counter)
    by_state_only_b: Counter[str] = field(default_factory=Counter)
    first: list[dict[str, Any]] = field(default_factory=list)


def rows(path: str) -> Iterator[dict[str, Any]]:
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                yield json.loads(line)


def replace_agents(value: Any, agents: dict[str, str]) -> Any:
    if isinstance(value, dict):
        return {k: replace_agents(v, agents) for k, v in value.items()}
    if isinstance(value, list):
        return [replace_agents(v, agents) for v in value]
    if isinstance(value, str) and value in agents:
        return agents[value]
    return value


def worlds(path: str) -> Iterator[tuple[dict[str, Any], World]]:
    """Yields (header, world) pairs; the header is the same object each time."""
    it = rows(path)
    header = next(it)
    current: dict[str, Any] | None = None
    body: list[dict[str, Any]] = []

    def finish() -> World:
        assert current is not None
        agents: dict[str, str] = {}
        partition: set[str] = set()
        unattributed = 0
        for r in body:
            if r["kind"] == "attribution":
                name = "x:" + ",".join(sorted(r["exchanges"]))
                agents[r["agent"]] = name
                partition.add(name)
            elif r["kind"] == "unattributed":
                unattributed += 1
                agents[r["agent"]] = f"unattributed:{unattributed}"
        tx: Counter[str] = Counter()
        for r in body:
            if r["kind"] == "transmission":
                canon = {k: v for k, v in r.items() if k != "id"}
                tx[json.dumps(replace_agents(canon, agents), sort_keys=True)] += 1
        return World(
            key=current["key"],
            status=json.dumps(current.get("status"), sort_keys=True),
            agents=agents,
            partition=partition,
            transmissions=tx,
            unattributed=unattributed,
        )

    for r in it:
        kind = r["kind"]
        if kind == "world":
            if current is not None:
                yield header, finish()
            current, body = r, []
        elif kind == "trailer":
            break
        else:
            body.append(r)
    if current is not None:
        yield header, finish()


def digest(s: str) -> str:
    return hashlib.blake2b(s.encode(), digest_size=8).hexdigest()


def state_of(canon: str) -> str:
    return str(json.loads(canon).get("state"))


def compare(a_path: str, b_path: str, first: int) -> tuple[Tally, dict[str, Any], dict[str, Any]]:
    t = Tally()
    pending_a: dict[str, World] = {}
    pending_b: dict[str, World] = {}
    ha: dict[str, Any] = {}
    hb: dict[str, Any] = {}

    def note(change: str, world: str, kind: str, ident: str) -> None:
        if len(t.first) < first:
            t.first.append({"change": change, "world": world, "kind": kind, "id": ident})

    def pair(wa: World, wb: World) -> None:
        t.worlds += 1
        equal = True
        t.attribution_rows_a += len(wa.partition)
        t.attribution_rows_b += len(wb.partition)
        t.unattributed_a += wa.unattributed
        t.unattributed_b += wb.unattributed
        t.transmissions_a += sum(wa.transmissions.values())
        t.transmissions_b += sum(wb.transmissions.values())
        if wa.status != wb.status:
            t.status_differs += 1
            equal = False
            note("changed", wa.key, "world", "status")
        if wa.partition != wb.partition or wa.unattributed != wb.unattributed:
            t.partition_differs += 1
            equal = False
            for p in sorted(wa.partition - wb.partition):
                note("only in a", wa.key, "attribution", digest(p))
            for p in sorted(wb.partition - wa.partition):
                note("only in b", wa.key, "attribution", digest(p))
        only_a = wa.transmissions - wb.transmissions
        only_b = wb.transmissions - wa.transmissions
        t.transmissions_matched += sum((wa.transmissions & wb.transmissions).values())
        t.transmissions_only_a += sum(only_a.values())
        t.transmissions_only_b += sum(only_b.values())
        for c, n in sorted(only_a.items()):
            t.by_state_only_a[state_of(c)] += n
            note("only in a", wa.key, "transmission", digest(c))
        for c, n in sorted(only_b.items()):
            t.by_state_only_b[state_of(c)] += n
            note("only in b", wa.key, "transmission", digest(c))
        if only_a or only_b:
            equal = False
        if equal:
            t.worlds_equal += 1

    ia = worlds(a_path)
    ib = worlds(b_path)
    done_a = done_b = False
    while not (done_a and done_b):
        wa = wb = None
        if not done_a:
            try:
                ha, wa = next(ia)
            except StopIteration:
                done_a = True
        if not done_b:
            try:
                hb, wb = next(ib)
            except StopIteration:
                done_b = True
        if wa is not None and wb is not None and wa.key == wb.key:
            pair(wa, wb)
            continue
        if wa is not None:
            if wa.key in pending_b:
                t.world_order_differs += 1
                pair(wa, pending_b.pop(wa.key))
            else:
                pending_a[wa.key] = wa
        if wb is not None:
            if wb.key in pending_a:
                t.world_order_differs += 1
                pair(pending_a.pop(wb.key), wb)
            else:
                pending_b[wb.key] = wb
    for k in pending_a:
        note("only in a", k, "world", "")
    for k in pending_b:
        note("only in b", k, "world", "")
    extra = {"worlds_only_a": len(pending_a), "worlds_only_b": len(pending_b)}
    return t, extra, {"a": ha.get("detector"), "b": hb.get("detector")}


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--run", required=True)
    p.add_argument("--a-label", default="golden")
    p.add_argument("--b-label", default="bench")
    p.add_argument("--out")
    p.add_argument("--first", type=int, default=20)
    args = p.parse_args()
    t, extra, detectors = compare(args.a, args.b, args.first)
    equal = (
        t.worlds == t.worlds_equal
        and extra["worlds_only_a"] == 0
        and extra["worlds_only_b"] == 0
    )
    summary = {
        "run": args.run,
        "a": args.a_label,
        "b": args.b_label,
        "result": "equal" if equal else "differs",
        "detectors": {args.a_label: detectors["a"], args.b_label: detectors["b"]},
        "worlds": t.worlds,
        "worlds_equal": t.worlds_equal,
        **extra,
        "world_order_differs": t.world_order_differs,
        "status_differs": t.status_differs,
        "partition_differs": t.partition_differs,
        "attribution_rows": {args.a_label: t.attribution_rows_a, args.b_label: t.attribution_rows_b},
        "unattributed_rows": {args.a_label: t.unattributed_a, args.b_label: t.unattributed_b},
        "transmissions": {
            args.a_label: t.transmissions_a,
            args.b_label: t.transmissions_b,
            "matched": t.transmissions_matched,
            f"only_{args.a_label}": t.transmissions_only_a,
            f"only_{args.b_label}": t.transmissions_only_b,
            f"only_{args.a_label}_by_state": dict(t.by_state_only_a),
            f"only_{args.b_label}_by_state": dict(t.by_state_only_b),
        },
        "first": t.first,
    }
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(json.dumps(summary, indent=2) + "\n")
    print(
        f"{args.run}: {summary['result']} worlds={t.worlds} equal={t.worlds_equal} "
        f"partition_differs={t.partition_differs} status_differs={t.status_differs} "
        f"tx a={t.transmissions_a} b={t.transmissions_b} matched={t.transmissions_matched} "
        f"only_a={t.transmissions_only_a} only_b={t.transmissions_only_b}"
    )
    return 0 if equal else 1


if __name__ == "__main__":
    sys.exit(main())
