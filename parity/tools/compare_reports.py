#!/usr/bin/env python3
"""Compare two score reports (ct-eval's or the bench's) count by count.

Usage: compare_reports.py <a/report.json | none> <b/report.json> --run NAME
                          [--a-label L] [--b-label L] [--out summary.json]

Either side may be a ct-eval `report.json` (crosstalk 7f8a2fb) or an
`a2a-bench score` report. Field-name differences (docs/features/score.md,
"Report shape") are normalised:

- `detector`: a string (ct-eval) or `{name, version, variant}` (bench);
  only the name is reported, never compared (it differs by design).
- transmission keys: ct-eval's `quality = {type, data: {...}}` becomes the
  bench's `{kind, ...}`.
- failures: ct-eval's `failures` list length vs the bench's
  `failed_worlds` (and its typed `failures` list length).
- `background.sources`: compared as the ordered list of (reason, count);
  the `text` field is never read.

Only counts, ratios, gate names/statuses/values and row keys are compared
or written. No dataset text (sources text, misses, false positives) is
read into the output. With `none` as the first report, only b's counts are summarised
(result `no-baseline`). Exit 0 when equal, 1 when any compared value differs.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from typing import Any

COUNT_SECTIONS = ("totals", "overall", "out_of_reach", "forwarding", "access_only")
ROW_KEY = ("dataset", "route", "carrier", "class", "tier")


@dataclass
class Comparison:
    differences: list[dict[str, Any]] = field(default_factory=list)
    compared: int = 0

    def check(self, path: str, a: Any, b: Any) -> None:
        self.compared += 1
        if a != b:
            self.differences.append({"path": path, "a": a, "b": b})


def detector_name(report: dict[str, Any]) -> str:
    det = report.get("detector")
    if isinstance(det, dict):
        return str(det.get("name"))
    return str(det)


def quality_key(quality: dict[str, Any]) -> str:
    if "type" in quality:
        flat = {"kind": quality["type"], **(quality.get("data") or {})}
    else:
        flat = dict(quality)
    return json.dumps(flat, sort_keys=True)


def counts_only(entry: dict[str, Any], key_fields: tuple[str, ...]) -> dict[str, Any]:
    return {k: v for k, v in entry.items() if k not in key_fields}


def rows_by_key(report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in report.get("rows", []):
        key = "/".join(str(row.get(k)) for k in ROW_KEY)
        if key in out:
            raise ValueError(f"duplicate row key {key}")
        out[key] = counts_only(row, ROW_KEY)
    return out


def transmissions_by_key(report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for t in report.get("transmissions", []):
        k = t["key"]
        key = f"{k.get('dataset')}/{k.get('route')}/{quality_key(k.get('quality', {}))}"
        if key in out:
            raise ValueError(f"duplicate transmission key {key}")
        out[key] = dict(t["counts"])
    return out


def violations_by_key(report: dict[str, Any]) -> dict[str, int]:
    return {f"{v['dataset']}/{v['reason']}": v["count"] for v in report.get("violations", [])}


def access_controls_by_key(report: dict[str, Any]) -> dict[str, int]:
    return {
        f"{v['dataset']}/{v['reason']}/{v['class']}": v["count"]
        for v in report.get("access_only_under_controls", [])
    }


def gates_by_name(report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {g["name"]: {"status": g.get("status"), "value": g.get("value")} for g in report.get("gates", [])}


def failed_worlds(report: dict[str, Any]) -> int:
    if "failed_worlds" in report:
        return int(report["failed_worlds"])
    return len(report.get("failures", []))


def sources(report: dict[str, Any]) -> list[list[Any]]:
    return [[s.get("reason"), s.get("count")] for s in (report.get("background") or {}).get("sources", [])]


def compare_maps(c: Comparison, prefix: str, a: dict[str, Any], b: dict[str, Any]) -> None:
    for key in sorted(set(a) | set(b)):
        c.check(f"{prefix}[{key}]", a.get(key), b.get(key))


def compare(a: dict[str, Any], b: dict[str, Any]) -> Comparison:
    c = Comparison()
    c.check("dataset", a.get("dataset"), b.get("dataset"))
    for section in COUNT_SECTIONS:
        sa, sb = a.get(section, {}), b.get(section, {})
        for k in sorted(set(sa) | set(sb)):
            c.check(f"{section}.{k}", sa.get(k), sb.get(k))
    compare_maps(c, "rows", rows_by_key(a), rows_by_key(b))
    compare_maps(c, "transmissions", transmissions_by_key(a), transmissions_by_key(b))
    compare_maps(c, "violations", violations_by_key(a), violations_by_key(b))
    compare_maps(c, "access_only_under_controls", access_controls_by_key(a), access_controls_by_key(b))
    compare_maps(c, "gates", gates_by_name(a), gates_by_name(b))
    c.check("failed_worlds", failed_worlds(a), failed_worlds(b))
    c.check("unscored", a.get("unscored"), b.get("unscored"))
    c.check(
        "unknown_detected_agent",
        a.get("unknown_detected_agent", 0),
        b.get("unknown_detected_agent", 0),
    )
    for k in ("false_positives", "exchanges", "per_1k_exchanges"):
        c.check(f"background.{k}", (a.get("background") or {}).get(k), (b.get("background") or {}).get(k))
    c.check("background.sources(reason,count)", sources(a), sources(b))
    c.check("misses.len", len(a.get("misses", [])), len(b.get("misses", [])))
    c.check("false_positives.len", len(a.get("false_positives", [])), len(b.get("false_positives", [])))
    return c


def headline(report: dict[str, Any]) -> dict[str, Any]:
    """Counts only: the figures the results table quotes."""
    gates = gates_by_name(report)
    status: dict[str, int] = {}
    for g in gates.values():
        status[str(g["status"])] = status.get(str(g["status"]), 0) + 1
    return {
        "detector": detector_name(report),
        "totals": report.get("totals"),
        "overall": report.get("overall"),
        "out_of_reach": report.get("out_of_reach"),
        "forwarding": report.get("forwarding"),
        "access_only": report.get("access_only"),
        "rows": len(report.get("rows", [])),
        "transmissions": len(report.get("transmissions", [])),
        "violations": violations_by_key(report),
        "access_only_under_controls": access_controls_by_key(report),
        "fp_per_1k": (report.get("background") or {}).get("per_1k_exchanges"),
        "false_positives": (report.get("background") or {}).get("false_positives"),
        "gates": status,
        "failed_worlds": failed_worlds(report),
        "unscored": report.get("unscored"),
        "unknown_detected_agent": report.get("unknown_detected_agent", 0),
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--run", required=True)
    p.add_argument("--a-label", default="baseline")
    p.add_argument("--b-label", default="bench")
    p.add_argument("--out")
    args = p.parse_args()
    if args.a == "none":
        with open(args.b, encoding="utf-8") as f:
            b = json.load(f)
        summary = {"run": args.run, "a": None, "b": args.b_label, "result": "no-baseline", args.b_label: headline(b)}
        if args.out:
            with open(args.out, "w", encoding="utf-8") as f:
                f.write(json.dumps(summary, indent=2) + "\n")
        print(f"{args.run}: no-baseline")
        return 0
    with open(args.a, encoding="utf-8") as f:
        a = json.load(f)
    with open(args.b, encoding="utf-8") as f:
        b = json.load(f)
    c = compare(a, b)
    summary = {
        "run": args.run,
        "a": args.a_label,
        "b": args.b_label,
        "result": "equal" if not c.differences else "differs",
        "compared_values": c.compared,
        "differences": c.differences,
        args.a_label: headline(a),
        args.b_label: headline(b),
    }
    text = json.dumps(summary, indent=2, sort_keys=False) + "\n"
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(text)
    print(f"{args.run}: {summary['result']} ({c.compared} values compared, {len(c.differences)} differ)")
    for d in c.differences:
        print(f"  {d['path']}: {args.a_label}={json.dumps(d['a'])} {args.b_label}={json.dumps(d['b'])}")
    return 0 if not c.differences else 1


if __name__ == "__main__":
    sys.exit(main())
