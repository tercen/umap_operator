# umap_rust_operator — notes for whoever works on this next

Built 2026-09-22 with the `create-rust-operator` skill, from the `flowsom_rust_operator`
scaffold. Read `README.md` first.

## Architecture

| file | does |
|---|---|
| `src/lib.rs` | `run` / `run_dev` / `execute`: read properties, gather the matrix, scale, draw the training set, fit, transform the rest, write, upload |
| `src/algorithm.rs` | uwot-style scaling, the seeded training draw (per sample or by fraction), row gathers |
| `src/props.rs` | properties with the R names; `manifest_declares_what_the_code_reads` asserts every one is in `operator.json` with the same default and that every `kind` is from the platform's vocabulary |
| `src/output.rs` | one table: `umap.1`, `umap.2`, `.ci` |
| `src/input.rs`, `context.rs`, `progress.rs`, `upload.rs`, `tson.rs`, `pagecache.rs` | verbatim from flowsom: chunked crosstab reader, task-only context, progress events, streamed `OperatorResult` upload |
| `src/main.rs`, `src/bin/dev.rs` | production (`--taskId`) and Studio (`--workflowId/--stepId`) entry points |

## The crosstab contract

Rows = channels, columns = cells (+ sample factor), y = value. The matrix is gathered
`n_cells × p` of `f64` — a transpose of how the crosstab arrives, scattered by `(.ri, .ci)`
with no order to rely on — and the memory model declares the cost.

## Memory

Gather 8 B/value; `umaprs` then holds an `f32` copy, the kNN (k × n × 12 B), the CSR graph and
the embedding. Below 2,000 training cells the spectral start does a **dense** eigendecomposition
(n² doubles plus LAPACK workspace): 149 MB and 11 s at 2,000 cells, which is why the constant
term is 165 MB and not 90 — a small projection is the *expensive* one per value. Measured in `STATUS.md`; refit `memory_model.json` from `stats_d_actual_ram_peak`
once there are real runs.

## The library

`umaprs` is pinned by **commit** to the parity branch (`tercen/umaprs#1`). Move the pin to a tag
once that PR is merged and tagged. It links OpenBLAS statically through `ndarray-linalg` for the
spectral start, which is why:

- the image is the **cc tier** (`distroless/cc`, glibc target) rather than `scratch`/musl;
- the builder installs `gfortran` and `make`;
- `Cargo.lock` pins `openblas-build`/`openblas-src` to **0.10.13**: 0.10.16 does not compile
  (`openblas-build requires the rustls or native-tls feature`). Re-pin after any `cargo update`.
- the image sets `OPENBLAS_NUM_THREADS=1`: OpenBLAS otherwise spins a thread per core for the
  spectral start's tiny eigenproblem — 14 s instead of 2 s on the 2,000-cell fixture. Set it
  on the host too when timing the dev binary.
- `umaprs` is MIT (licence added on the parity branch; the crate had none). This operator is
  AGPL-3.0 like the other Tercen operators.

## Deliberate differences from the R operator

- A negative `seed` is refused (R reads it as "random").
- `pca` with a training draw is refused: the projection is not stored on the model, so the
  remaining cells could not be transformed. Fit on every cell, or drop `pca`.
- `init` offers four values; uwot's variants are mapped (see README).
- `prop.train` draws `ceiling(n × prop.train)` cells as R does, seeded with ChaCha8 rather than
  R's Mersenne-Twister — the draw is repeatable, not identical to R's.

## Seed and RNG

Training draw: `ChaCha8Rng::seed_from_u64(seed)`. Layout and SGD: `umaprs` `random_state(seed)`.
`threads = 1` reproduces bit-for-bit; above 1 the HogWild SGD and the parallel HNSW build make
the last digits scheduling-dependent.

## Tercen unit test

`tests/test.json`: public 2,000-cell subset of Levine-32 AML (from `umaprs/data`), two synthetic
samples of 1,000, `train_cells_per_sample = 500`, `threads = 1`, `seed = 42`. Golden exported from a
Studio dev run (project "umap_rust_operator dev", 2026-09-22). It guards the join and the shape and
the coordinates at `equalityMethod: R2`; a different `umaprs` commit is expected to move the
coordinates, in which case regenerate the golden and say so in the commit.

## Dev loop

```
. dev/studio.env            # TERCEN_URI (gRPC, container IP:50051), TERCEN_HTTP, TERCEN_TOKEN — git-ignored
python dev/setup_umap.py <workflowId> <tableSchemaId> train_cells_per_sample=500 sample_factor=sample threads=1
PROTOC=… cargo build --release && target/release/dev --workflowId … --stepId …
```

## Release

Bump `container` in `operator.json` and `version` in `Cargo.toml`, let CI go green, tag `X.Y.Z`,
push the tag. The pin and the tag are one change; never leave `main` pinned to an untagged version.
The GHCR package is private on first publish; making it public is a manual flip.
