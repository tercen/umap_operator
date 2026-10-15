//! Peak memory of a full UMAP fit, for refitting `memory_model.json`.
//!
//! `cargo run --release --example fit_memory -- <cells> <channels>` builds a synthetic mixture
//! (20 Gaussian clusters), shapes it the way `lib.rs` does and fits with the operator's
//! defaults. Run it under `/usr/bin/time -v` and read "Maximum resident set size".
use ndarray::Array2;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::time::Instant;

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let (n_cells, p) = (args[0], args[1]);
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let centres: Vec<Vec<f64>> = (0..20)
        .map(|_| (0..p).map(|_| rng.gen_range(-5.0..5.0)).collect())
        .collect();
    let mut data = Vec::with_capacity(n_cells * p);
    for _ in 0..n_cells {
        let c = &centres[rng.gen_range(0..centres.len())];
        for &m in c {
            data.push(m + rng.gen_range(-1.0..1.0));
        }
    }
    let m = Array2::from_shape_vec((n_cells, p), data).unwrap();
    let t = Instant::now();
    let emb = umaprs::UMAP::new()
        .n_neighbors(15)
        .min_dist(0.2)
        .init(umaprs::InitMethod::Auto)
        .random_state(42)
        .threads(0)
        .fit_transform(&m);
    eprintln!(
        "{n_cells} x {p}: fit {:.1} s, embedding {:?}",
        t.elapsed().as_secs_f64(),
        emb.dim()
    );
}
