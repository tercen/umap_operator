# Status


## 2.0.2 (2026-10-16): umaprs 0.2.2

- **One-CPU speed.** umaprs's exact kd-tree (≤ 16 channels) was 58% of a one-core fit; rebuilt, it
  is 15.6× faster at 695,052 × 8 (869 s → 55.6 s) with the same neighbours (3 rows differ only by
  exactly tied distances). tercen.com, 2.0.1, one CPU: 814 s against the R operator's 431 s.
- **`init = spectral` above 2,000 cells was a random start** (every step upgraded from 1.x carries
  `spectral`). Now the PCA start, as `auto` and as uwot's `spca` fallback; the task log names the
  start. Measured against uwot (the R operator's call) on public Levine-13/32: see the morning
  report of 2026-10-16.

## 0.1.2 (2026-09-22)

- **No more OpenBLAS.** `umaprs` now does its symmetric eigendecompositions in pure Rust
  (nalgebra), so the image is back on the static tier (musl on `scratch`). The weekly no-cache
  CI build had failed inside OpenBLAS's own `make` on an AVX-512 runner, and a build that
  succeeds is tuned to the runner's CPU — a shipped binary must not depend on which machine
  GitHub handed out. Spectral start at 2,000 cells: 13 s (LAPACK: 11 s).
- Golden regenerated: the eigenvectors are the same subspace but not the same numbers.
- Platform test cut to the two relations the assembled result actually has (0.1.1's first
  install on an instance said `bad.nRelations` for the three-table copy of flowsom's test).

## 0.1.1 (2026-09-22)

- A training draw smaller than `n_neighbors` is refused (it was accepted and produced a layout of
  ten cells).
- Constant memory term 90 → 165 MB: below 2,000 training cells the spectral start does a dense
  eigendecomposition — 149 MB at 2,000 cells, above the 98 MB the 0.1.0 model booked for that
  projection.
- `umaprs` pinned to tag 0.2.0 (tercen/umaprs#1 merged, Apache-2.0; code identical to the earlier pin). Operator relicensed AGPL-3.0 → Apache-2.0 on 2026-10-08.
- Shape matrix run end to end on Studio: no sample factor + draw (one-sample rule, logged),
  `prop.train`, `scale = Z` + `spectral`, `pca` full fit, `pca` + draw (refused), tiny draw
  (now refused), `n_neighbors` > cells (refused), bad `init` (refused).

## 0.1.0 (2026-09-22)

First release. Rows = channels, columns = cells (+ sample), one embedding table back.

### Measured

Dev runs against local Studio, `/usr/bin/time -v` under the worker's `MALLOC_CONF`, 16 cores:

| projection | settings | wall | peak RSS |
|---|---|---|---|
| 2,000 cells × 38 (76k values) | draw 500/sample of 2, `threads = 1` | 2.1 s | 57 MB |
| 50,500 cells × 38 (1.92 M values) | draw 5,000/sample of 4 | 4.4 s | 105 MB |
| 50,500 cells × 38 (1.92 M values) | fit on every cell | — | 177 MB |

The full fit is the expensive shape (the kNN and graph cover every cell): 65 B per crosstab value
above a 52 MB floor. A training draw halves it.

### Reproducibility

Two dev runs of the same step at `threads = 1`, exported through the server, are byte-identical.
They were not at first: `umaprs`'s `transform` built its kNN index on the global rayon pool
regardless of the fit's thread count. Fixed in the library (the model now carries `threads`);
this is what pinning by commit is for.

### The platform test

`tests/test.json` (2,000 public AML cells, two synthetic samples, `train_cells_per_sample = 500`,
`threads = 1`, `absTol` 1e-6 against a Studio export of the server's own reading of the result)
**passed on 0.1.1 installed on local Studio** (`TEST -- umap_draw500_per_sample_threads1
successful`). 0.1.0's run of it failed on shape — three output tables listed where the assembled
result has two — which is exactly the class of error only this test can see. The golden is
regenerated for 0.1.2 (different eigensolver, same subspace, different numbers).

### Memory model

`intercept × n_cols + 1.5 × offset`, MB, where `n_cols` is the number of cells (the column table).
**2.0.1** books `0.004` MB per cell and `offset` 140 (210 MB constant). 2.0.0 booked per value
(`0.0001` × `n_main`), fitted on 38-channel data; with few channels that is far too little,
because a full fit's cost is per cell. On tercen.com (2026-10-15) a full fit of 695,052 cells ×
8 channels booked 756 MB and was killed at 794 MB after 620 s.

Full-fit peaks, `examples/fit_memory.rs` under `/usr/bin/time -v` (16 cores, synthetic mixture):

| cells × channels | fit | peak RSS |
|---|---|---|
| 100,000 × 8 | 9.3 s | 210 MB |
| 300,000 × 8 | 34.9 s | 607 MB |
| 695,052 × 8 | 101.8 s | 1,435 MB |
| 200,000 × 40 | 33.6 s | 517 MB |

About 2.06 KB per cell at 8 channels plus ~17 B per extra channel per cell. The model books
4 KB per cell to cover reading and writing and up to ~100 channels: 695k cells book 2.99 GB, a
1.6 M-cell cohort 6.6 GB. A training draw (`prop.train` < 1) needs less; the model cannot see the
property's effect, so it books for the full fit. The test `memory_model_books_a_full_fit_by_cells`
pins the measured points.

### CPUs in production: booked from the projection, and the operator uses what it gets

The platform books an operator task's CPUs from the size of its crosstab: `ceil(values /
tercen.query.rows.per.cpu)` with a default of 10 M values per CPU, capped by `tercen.task.max.cpus`
(4 on tercen.com), the same estimate that sizes the cube query; a step can also set CPUs by
hand. So a 9 M-value projection runs on **one CPU**, a 45 M-value one on **four** (both seen on
tercen.com, 2026-09-22, `stats_d_estimated_cpu`), and the runner enforces it as a podman `--cpus`
quota with `OPENBLAS_NUM_THREADS` to match. `umaprs` is parallel throughout and rayon honours the
quota, so nothing needs configuring: `threads = 0` is right in production.

Measured on tercen.com, 220,000 cells × 41 channels (9 M values), same input: **96 s at 4 CPUs,
196 s at 1 CPU** — 2.0× from 4× the cores, because a platform task carries 60–70 s of serial
work (streaming the crosstab out, upload, ingestion) that does not scale; the compute itself
scaled ~3× (locally the 50k full fit went 29 s → 10 s from 1 to 4 CPUs). In the image on the
50,500-cell full fit: 29 s at `--cpus 1`, 10 s at `--cpus 4`, 4.4 s unlimited on 16 cores. At
cohort scale on one core (465k fit + 1.2 M transform at 40 dims, `taskset -c 0`): 525 s to fit
and 758 s to project.

### Left out

`pca` with a training draw (the projection is not on the model); 3-D; densMAP; NNDescent.
