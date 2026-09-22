# Status

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
- `umaprs` pin moved to the commit that adds its MIT licence (code identical).
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

`intercept × n_main + 1.5 × offset`, MB. Fitted to the two full-fit points above — 65 B/value, 52 MB — and booked with headroom as
`0.0001` MB/value and `offset` 60 (so 90 MB constant): 1.92 M values book 282 MB against a
measured 177; a 1.6 M-cell × 40-channel cohort books 6.5 GB. Refit from `stats_d_actual_ram_peak`
after real runs.

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
