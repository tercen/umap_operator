# Status

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

### The platform test has not run on an instance yet

`tercenctl operator install` pulls the image from GHCR even when the same tag is loaded in the
instance's podman store, and a package published from a private repository starts private, so
the `OperatorUnitTest` in `tests/` (2,000 public AML cells, `train_cells_per_sample = 500`,
`threads = 1`, `absTol` 1e-6 against a Studio export) runs the first time the package is made
public and the operator installed. The golden was cut from the server's own reading of the
result, and two runs of it were byte-identical, so the test is expected to pass; it is still
untested as a *test*.

### Memory model

`intercept × n_main + 1.5 × offset`, MB. Fitted to the two full-fit points above — 65 B/value, 52 MB — and booked with headroom as
`0.0001` MB/value and `offset` 60 (so 90 MB constant): 1.92 M values book 282 MB against a
measured 177; a 1.6 M-cell × 40-channel cohort books 6.5 GB. Refit from `stats_d_actual_ram_peak`
after real runs.

### One CPU in production, and what that costs

The platform books **one CPU** for an operator task unless the task carries an explicit CPU
request (`task_service.dart`: `cpus = 1.0` when none is set; the runner turns it into a podman
`--cpus` quota and `OPENBLAS_NUM_THREADS`). `umaprs` is parallel throughout and rayon honours
the quota, so nothing needs configuring: the operator uses what it is given. Measured in the
image on the 50,500-cell full fit: **29 s at `--cpus 1`, 10 s at `--cpus 4`**, 4.4 s unlimited on
16 cores. At cohort scale on one core (465k fit + 1.2 M transform at 40 dims, `taskset -c 0`):
TODO_ONE_CPU. Acceptable for a batch step; a task booked more CPUs gets the speed-up for free.

### Left out

`pca` with a training draw (the projection is not on the model); 3-D; densMAP; NNDescent.
