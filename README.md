# Selection Generator Heuristic Experiments

This repository is a thesis fork of
[`JGDoerrer/selection_generator`](https://github.com/JGDoerrer/selection_generator).
The upstream project searches for optimal comparison algorithms for selection.
This fork keeps the original forward, backward, and bidirectional searches and
adds thesis experiments for fixed heuristic outcome rules in the forward search.

The original upstream README is preserved as
[`README_UPSTREAM.md`](README_UPSTREAM.md). This file documents the current
thesis artifact.

## What Changed

- Heuristic path search modes in [`src/search_forward.rs`](src/search_forward.rs)
- CLI support for heuristic runs in [`src/main.rs`](src/main.rs)
- Heuristic result logs under [`logs/heuristic`](logs/heuristic)
- Summary table generator: [`scripts/summarize_heuristics.py`](scripts/summarize_heuristics.py)

The code uses 0-based ranks. For example, `n = 12, i = 4` means selecting the
5th smallest element in 1-based mathematical notation.

## Build

Requirements are the same as upstream: Rust, a C compiler, and `nauty` through
the `nauty-Traces-sys` crate.

```sh
cargo build --release
```

## Run Exact Search

Forward search:

```sh
cargo run --release -- \
  --search-mode forward \
  --print-algorithm \
  --single \
  -n 12 \
  -i 4
```

Backward search:

```sh
cargo run --release -- \
  --search-mode backward \
  --max-core 16 \
  --print-algorithm \
  --single \
  -n 12 \
  -i 4
```

## Run Heuristics

Fast heuristic search:

```sh
cargo run --release -- \
  --search-mode heuristic \
  --heuristic-fast \
  --heuristic-strategy 4 \
  --single \
  -n 9 \
  -i 2
```

Tracked heuristic search:

```sh
cargo run --release -- \
  --search-mode heuristic \
  --heuristic-strategy 4 \
  --single \
  -n 9 \
  -i 2
```

Limit the cache to 1 GiB:

```sh
cargo run --release -- \
  --search-mode heuristic \
  --heuristic-fast \
  --heuristic-strategy 4 \
  --max-cache-size 1073741824 \
  --single \
  -n 9 \
  -i 2
```

Without `--single`, the program continues from the requested `(n, i)` through
the remaining ranks and values of `n`.

Current heuristic strategies:

| Strategy | Name |
| --- | --- |
| `0` | Transitive-closure product |
| `1` | Hasse-edge product |
| `2` | Total-relations minimization |
| `3` | Rank-candidate maximization |
| `4` | Compatible-rank-prefix maximization |
| `5` | Downset-delta minimization |
| `6` | Fixed `a < b` baseline |

Heuristic runs write to:

```text
logs/heuristic/strategy_<k>/fast/results.csv
logs/heuristic/strategy_<k>/tracked/results.csv
```

Tracked runs also write final poset exports under:

```text
logs/heuristic/strategy_<k>/tracked/runs/
```

## Summaries

Generate the summary table for a fixed 0-based rank:

```sh
python3 scripts/summarize_heuristics.py --i 2
python3 scripts/summarize_heuristics.py --i 3
```

Default outputs:

```text
logs/heuristic/summary/summary_i_2.csv
logs/heuristic/summary/summary_i_3.csv
```

The summary script reads `fast` results by default. It includes the current
strategies `0` through `6`, keeps the last result for each `(strategy, n, i)`,
and appends `runtime_sum` and `mean_ratio_to_exact` rows.

Useful filters:

```sh
python3 scripts/summarize_heuristics.py --i 3 --n-min 7 --n-max 15
python3 scripts/summarize_heuristics.py --i 3 --include-strategies 0 1 2 3 4 5 6
python3 scripts/summarize_heuristics.py --i 3 --output-dir logs/heuristic/summary/final_i3
```

When `--output-dir` is provided, the script writes `fast_table.csv` inside that
directory.

## Validate Generated Algorithms

Validate one generated algorithm by copying it to
[`src/algorithm_test/algorithm.rs`](src/algorithm_test/algorithm.rs) and running:

```sh
cargo test algorithm_test --release
```

Validate all algorithms in [`algorithms`](algorithms):

```sh
sh test_algorithms.sh
```

The upstream backward-search algorithms are stored under:

```text
logs/backward/algorithms
```

## Notes

The current code uses compact heuristic numbering `0..=6`. Older result files
from before the removal of the old two-pool heuristic may use the previous
numbering, where compatible prefixes, downset delta, and the fixed baseline
were stored as strategies `5`, `6`, and `7`. Regenerate summaries from current
runs when comparing thesis tables.

## Provenance And License

This fork is based on upstream commit ancestry from
[`JGDoerrer/selection_generator`](https://github.com/JGDoerrer/selection_generator).
The original MIT license is preserved in [`LICENSE`](LICENSE).
