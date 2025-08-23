#!/usr/bin/env python3
import argparse
import glob
import json
import os
import sys
from collections import defaultdict
from datetime import datetime
from typing import Dict, Tuple, Any

Key = Tuple[str, int, int, int, str]


def find_latest_results() -> str:
    candidates = glob.glob(os.path.join("_tgt", "tests", "*", "results.jsonl"))
    if not candidates:
        return ""
    candidates.sort(key=lambda p: os.path.getmtime(p), reverse=True)
    return candidates[0]


def load_jsonl(path: str):
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except Exception:
                # ignore malformed lines
                continue


def key_from_record(rec: Dict[str, Any]) -> Key:
    scenario = str(rec.get("scenario", ""))
    k = int(rec.get("k", 0))
    pct = int(rec.get("pct", 0))
    chunk = int(rec.get("chunk", 0))
    il = str(rec.get("interleave", ""))
    return (scenario, k, pct, chunk, il)


def summarize(path: str) -> Dict[str, Any]:
    groups: Dict[Key, Dict[str, Any]] = {}
    totals = {"runs": 0, "passes": 0, "fails": 0}

    for rec in load_jsonl(path):
        # Only count final outcomes (if present), else count stage==verify or ok field
        ok = rec.get("ok")
        if ok is None:
            # fall back to stage == verify outcome if available
            if rec.get("stage") not in ("verify", "final"):
                continue
            ok = bool(rec.get("ok", False))
        key = key_from_record(rec)
        if key not in groups:
            scenario, k, pct, chunk, il = key
            groups[key] = {
                "scenario": scenario,
                "k": k,
                "pct": pct,
                "chunk": chunk,
                "interleave": il,
                "runs": 0,
                "passes": 0,
                "fails": 0,
                "examples": [],
            }
        g = groups[key]
        g["runs"] += 1
        totals["runs"] += 1
        if ok:
            g["passes"] += 1
            totals["passes"] += 1
        else:
            g["fails"] += 1
            totals["fails"] += 1
            # capture minimal repro info
            g["examples"].append({
                "timestamp": rec.get("timestamp"),
                "stage": rec.get("stage"),
                "error": rec.get("error"),
            })

    rows = list(groups.values())
    rows.sort(key=lambda r: (r["scenario"], r["k"], r["pct"], r["chunk"], r["interleave"]))

    summary = {
        "path": path,
        "generated": datetime.utcnow().isoformat() + "Z",
        "totals": totals,
        "groups": rows,
    }
    return summary


def print_table(summary: Dict[str, Any]) -> None:
    print("Summary for:", summary["path"])  # one-liner
    print("Totals: runs=%d passes=%d fails=%d" % (summary["totals"]["runs"], summary["totals"]["passes"], summary["totals"]["fails"]))
    print("scenario  k  pct  chunk     il   runs  pass  fail")
    for g in summary["groups"]:
        print("%8s %2d %3d %8d %4s %6d %5d %5d" % (
            g["scenario"], g["k"], g["pct"], g["chunk"], g["interleave"], g["runs"], g["passes"], g["fails"],
        ))


def main():
    ap = argparse.ArgumentParser(description="Summarize ParXive harness JSONL results")
    ap.add_argument("--path", help="Path to results.jsonl (default: latest)")
    ap.add_argument("--out", help="Write JSON summary to this file (default: alongside results.jsonl)")
    args = ap.parse_args()

    path = args.path or find_latest_results()
    if not path:
        print("No results.jsonl found under _tgt/tests", file=sys.stderr)
        return 2

    summary = summarize(path)

    out_path = args.out
    if not out_path:
        out_dir = os.path.dirname(path)
        out_path = os.path.join(out_dir, "summary.json")
    try:
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(summary, f, indent=2, sort_keys=False)
    except Exception as e:
        print(f"Failed to write summary to {out_path}: {e}", file=sys.stderr)

    print_table(summary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
