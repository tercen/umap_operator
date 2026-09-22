# UMAP (Rust) — Tercen operator

Uniform Manifold Approximation and Projection of cells, as a drop-in for the R `umap_operator`,
without the R runtime. The library is [`umaprs`](https://github.com/tercen/umaprs), which
reproduces `umap-learn 0.5.12`'s graph and transform stage by stage (see its `STATUS.md`).

## Projection

| | |
|---|---|
| rows | channel (or variable) name |
| columns | cell / event id, optionally with the sample it belongs to as a second column factor |
| y | the value |

Every cell needs a value for every channel. Rows with a missing value fail the run with a
message saying how many.

## Output

One table, one row per cell, joined on the column (observation): `umap.1`, `umap.2` (double).

## Properties

| name | default | meaning |
|---|---|---|
| `init` | `auto` | starting layout: `auto` (spectral below 2,000 cells, PCA above), `spectral`, `pca`, `random`. uwot's `normlaplacian`/`laplacian`/`agspectral` are read as `spectral`, `spca` as `pca`, `lvrandom` as `random` |
| `scale` | `none` | `none`, `Z`, `maxabs`, `range`, `colrange` — uwot's meanings |
| `n_neighbors` | 15 | neighbourhood size |
| `spread` | 1 | effective scale of the embedding |
| `min_dist` | 0.5 | effective minimum distance (the R operator's default; cytometry pipelines often use 0.01) |
| `pca` | −1 | reduce to this many components first when positive; not with a training draw |
| `prop.train` | 1 | fraction of cells to fit on; the rest are projected |
| `train_cells_per_sample` | 0 | fit on this many cells from **every** sample and project the rest; overrides `prop.train` |
| `sample_factor` | | the column factor naming the sample; empty takes the first, unless that has one value per cell |
| `seed` | 42 | non-negative; drives the draw, the layout and the optimisation |
| `n_epochs` | 0 | 0 = 500 below 10,000 cells, 200 above |
| `threads` | 0 | 0 = every core given; 1 = bit-for-bit reproducible |

## Fit on a draw, project the rest

`train_cells_per_sample = 5000` on a cohort of 200 files fits the model on 1 M cells and places
the remaining cells by `transform`, as `umap-learn` and `uwot` do. Each file is represented in the
model whatever its size, which `prop.train` cannot do. At 465k training cells and 1.2 M
projected (40 channels, 16 cores) `umaprs` takes 96 s to fit and 104 s to project.

## Parity

Not bitwise with R (`uwot`), and not meant to be: stochastic layouts differ between any two
implementations. `umaprs` is inside `umap-learn`'s and `uwot`'s own seed-to-seed spread on kNN
purity, trustworthiness and neighbour stability on public AML data; the tables are in its
`STATUS.md`. `tests/test.json` guards the platform join and the shape of the result on a public
2,000-cell subset of Levine-32 AML, with an R² tolerance on the coordinates.

## Threads and the platform

The platform books one CPU per operator task unless the task carries an explicit CPU request,
and enforces it as a quota. `umaprs` is parallel and honours the quota, so `threads = 0` (the
default) is right in production and gets more when more is booked. Measured: the 50,500-cell
full fit takes 29 s on one CPU, 10 s on four. `threads = 1` is for bit-for-bit reproduction.
