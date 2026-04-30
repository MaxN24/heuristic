#!/usr/bin/env python3

import argparse
import csv
from pathlib import Path


KNOWN_VALUES = [
    [0],
    [0],
    [1],
    [2, 3],
    [3, 4],
    [4, 6, 6],
    [5, 7, 8],
    [6, 8, 10, 10],
    [7, 9, 11, 12],
    [8, 11, 12, 14, 14],
    [9, 12, 14, 15, 16],
    [10, 13, 15, 17, 18, 18],
    [11, 14, 17, 18, 19, 20],
    [12, 15, 18, 20, 21, 22, 23],
    [13, 16, 19, 21, 23, 24, 25],
    [14, 17, 20, 23, 24, 26, 26, 27],
    [15, 18, 21, 24, 26, 27],
]

CURRENT_STRATEGIES = [0, 1, 2, 3, 4, 5, 6]


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", default="fast")
    parser.add_argument("--i", type=int, required=True)
    parser.add_argument("--n-min", type=int, default=None)
    parser.add_argument("--n-max", type=int, default=None)
    parser.add_argument("--logs-root", default="logs/heuristic")
    parser.add_argument("--output-dir", default=None)
    parser.add_argument("--include-strategies", type=int, nargs="*", default=CURRENT_STRATEGIES)
    parser.add_argument("--exclude-strategies", type=int, nargs="*", default=[])
    return parser.parse_args()


def read_results(logs_root, mode):
    rows = {}

    for path in sorted(logs_root.glob("strategy_*/" + mode + "/results.csv")):
        strategy_text = path.parent.parent.name.replace("strategy_", "")
        strategy = int(strategy_text)

        with path.open(newline="", encoding="utf-8") as handle:
            reader = csv.DictReader(handle)
            for row in reader:
                if not row:
                    continue

                n_text = (row.get("n") or "").strip()
                i_text = (row.get("i") or "").strip()
                comparisons_text = (row.get("total_comparisons") or "").strip()
                seconds_text = (row.get("elapsed_seconds") or "").strip()

                if n_text == "" or i_text == "":
                    continue

                n = int(n_text)
                i = int(i_text)
                comparisons = int(comparisons_text) if comparisons_text != "" else None
                seconds = float(seconds_text) if seconds_text != "" else None

                rows[(strategy, n, i)] = {
                    "strategy": strategy,
                    "n": n,
                    "i": i,
                    "comparisons": comparisons,
                    "seconds": seconds,
                }

    return list(rows.values())


def keep_row(row, args):
    if row["i"] != args.i:
        return False
    if args.n_min is not None and row["n"] < args.n_min:
        return False
    if args.n_max is not None and row["n"] > args.n_max:
        return False
    if row["strategy"] not in args.include_strategies:
        return False
    if row["strategy"] in args.exclude_strategies:
        return False
    return True


def exact_value(n, i):
    if n < 0 or n >= len(KNOWN_VALUES):
        return None
    if i < 0 or i >= len(KNOWN_VALUES[n]):
        return None
    return KNOWN_VALUES[n][i]


def format_seconds(value):
    if value is None:
        return ""
    return f"{value:.6f}"


def format_duration(seconds):
    if seconds is None:
        return ""

    days = int(seconds // 86400)
    seconds -= days * 86400
    hours = int(seconds // 3600)
    seconds -= hours * 3600
    minutes = int(seconds // 60)
    seconds -= minutes * 60

    parts = []
    if days:
        parts.append(str(days) + "d")
    if days or hours:
        parts.append(str(hours) + "h")
    if days or hours or minutes:
        parts.append(str(minutes) + "m")
    parts.append(f"{seconds:.1f}s")
    return " ".join(parts)


def write_table(rows, output_path, strategies):
    problems = sorted(set((row["n"], row["i"]) for row in rows))

    by_key = {}
    for row in rows:
        by_key[(row["strategy"], row["n"], row["i"])] = row

    header = ["n", "i"]
    for strategy in strategies:
        header.append("h" + str(strategy) + "_comparisons")
        header.append("h" + str(strategy) + "_elapsed_seconds")

    total_seconds = {}
    ratio_sum = {}
    ratio_count = {}

    for row in rows:
        strategy = row["strategy"]

        if row["seconds"] is not None:
            total_seconds[strategy] = total_seconds.get(strategy, 0.0) + row["seconds"]

        exact = exact_value(row["n"], row["i"])
        if exact is not None and exact != 0 and row["comparisons"] is not None:
            ratio_sum[strategy] = ratio_sum.get(strategy, 0.0) + row["comparisons"] / exact
            ratio_count[strategy] = ratio_count.get(strategy, 0) + 1

    with output_path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(header)

        for n, i in problems:
            output_row = [n, i]
            for strategy in strategies:
                row = by_key.get((strategy, n, i))
                if row is None:
                    output_row.append("")
                    output_row.append("")
                else:
                    output_row.append("" if row["comparisons"] is None else row["comparisons"])
                    output_row.append(format_seconds(row["seconds"]))
            writer.writerow(output_row)

        output_row = ["runtime_sum", ""]
        for strategy in strategies:
            output_row.append("")
            output_row.append(format_duration(total_seconds.get(strategy)))
        writer.writerow(output_row)

        output_row = ["mean_ratio_to_exact", ""]
        for strategy in strategies:
            if ratio_count.get(strategy, 0) == 0:
                output_row.append("")
            else:
                output_row.append(f"{ratio_sum[strategy] / ratio_count[strategy]:.3f}")
            output_row.append("")
        writer.writerow(output_row)


def main():
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[1]
    logs_root = repo_root / args.logs_root

    if args.output_dir is None:
        output_dir = logs_root / "summary"
        output_name = "summary_i_" + str(args.i) + ".csv"
    else:
        output_dir = repo_root / args.output_dir
        output_name = args.mode + "_table.csv"

    output_dir.mkdir(parents=True, exist_ok=True)
    output_path = output_dir / output_name

    rows = read_results(logs_root, args.mode)
    rows = [row for row in rows if keep_row(row, args)]
    rows.sort(key=lambda row: (row["n"], row["i"], row["strategy"]))

    if not rows:
        raise SystemExit("No matching rows found.")

    strategies = set()
    for strategy in args.include_strategies:
        if strategy not in args.exclude_strategies:
            strategies.add(strategy)
    strategies = sorted(strategies)

    write_table(rows, output_path, strategies)
    print("Wrote " + str(output_path))


if __name__ == "__main__":
    main()
