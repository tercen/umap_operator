//! The parts of the computation that are not UMAP itself: scaling the matrix the way uwot
//! does, choosing the training draw, and putting projected cells back in `.ci` order.
//! Pure functions over slices; no Tercen types.
use anyhow::{Result, bail};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;

use crate::props::Scale;

/// Scale `data` (`n_cells × p`, cell-major) in place, with uwot's `scale_input` semantics.
/// A channel with no spread is left centred rather than divided by zero.
pub fn scale_in_place(data: &mut [f64], n_cells: usize, p: usize, how: Scale) {
    if n_cells == 0 || p == 0 {
        return;
    }
    let column_mean = |data: &[f64], j: usize| -> f64 {
        data.iter().skip(j).step_by(p).sum::<f64>() / n_cells as f64
    };
    match how {
        Scale::None => {}
        Scale::Z => {
            for j in 0..p {
                let m = column_mean(data, j);
                let var = data
                    .iter()
                    .skip(j)
                    .step_by(p)
                    .map(|v| (v - m) * (v - m))
                    .sum::<f64>()
                    / (n_cells.max(2) - 1) as f64;
                let sd = if var > 0.0 { var.sqrt() } else { 1.0 };
                data.iter_mut()
                    .skip(j)
                    .step_by(p)
                    .for_each(|v| *v = (*v - m) / sd);
            }
        }
        Scale::MaxAbs => {
            for j in 0..p {
                let m = column_mean(data, j);
                data.iter_mut().skip(j).step_by(p).for_each(|v| *v -= m);
            }
            let max_abs = data.iter().fold(0.0f64, |a, v| a.max(v.abs()));
            if max_abs > 0.0 {
                data.iter_mut().for_each(|v| *v /= max_abs);
            }
        }
        Scale::Range => {
            let (lo, hi) = data
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                    (l.min(*v), h.max(*v))
                });
            if hi > lo {
                data.iter_mut().for_each(|v| *v = (*v - lo) / (hi - lo));
            }
        }
        Scale::ColRange => {
            for j in 0..p {
                let (lo, hi) = data
                    .iter()
                    .skip(j)
                    .step_by(p)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                        (l.min(*v), h.max(*v))
                    });
                if hi > lo {
                    data.iter_mut()
                        .skip(j)
                        .step_by(p)
                        .for_each(|v| *v = (*v - lo) / (hi - lo));
                }
            }
        }
    }
}

/// Which cells to fit on. `None` means all of them.
///
/// `per_sample > 0` draws that many cells from every group (all of a group smaller than that);
/// otherwise `prop_train < 1` draws that fraction of all cells, as the R operator does
/// (`ceiling(n * prop.train)`). Draws are seeded and sorted, so the same input gives the same
/// training set.
pub fn train_indices(
    n_cells: usize,
    groups: &[usize],
    per_sample: usize,
    prop_train: f64,
    seed: u64,
) -> Result<Option<Vec<usize>>> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    if per_sample > 0 {
        if groups.len() != n_cells {
            bail!(
                "train_cells_per_sample needs a sample per cell; got {} group labels for \
                 {n_cells} cells",
                groups.len()
            );
        }
        let n_groups = groups.iter().copied().max().map_or(0, |m| m + 1);
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); n_groups];
        for (i, &g) in groups.iter().enumerate() {
            members[g].push(i);
        }
        let mut out = Vec::new();
        for m in members.iter_mut() {
            if m.len() > per_sample {
                m.shuffle(&mut rng);
                m.truncate(per_sample);
            }
            out.extend_from_slice(m);
        }
        out.sort_unstable();
        if out.len() == n_cells {
            return Ok(None);
        }
        return Ok(Some(out));
    }
    if prop_train >= 1.0 {
        return Ok(None);
    }
    let size = ((n_cells as f64) * prop_train).ceil() as usize;
    let size = size.clamp(1, n_cells);
    if size == n_cells {
        return Ok(None);
    }
    let mut idx: Vec<usize> = (0..n_cells).collect();
    idx.shuffle(&mut rng);
    idx.truncate(size);
    idx.sort_unstable();
    Ok(Some(idx))
}

/// The complement of a sorted index list in `0..n`.
pub fn complement(sorted: &[usize], n: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(n - sorted.len());
    let mut k = 0;
    for i in 0..n {
        if k < sorted.len() && sorted[k] == i {
            k += 1;
        } else {
            out.push(i);
        }
    }
    out
}

/// Rows of `data` (`n × p`) at `idx`, as a new cell-major matrix.
pub fn take_rows(data: &[f64], p: usize, idx: &[usize]) -> Vec<f64> {
    let mut out = Vec::with_capacity(idx.len() * p);
    for &i in idx {
        out.extend_from_slice(&data[i * p..(i + 1) * p]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn z_scaling_centres_and_unit_scales_each_channel() {
        let mut d = vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0];
        scale_in_place(&mut d, 3, 2, Scale::Z);
        assert!((d[0] + 1.0).abs() < 1e-12 && d[2].abs() < 1e-12 && (d[4] - 1.0).abs() < 1e-12);
        assert!((d[1] + 1.0).abs() < 1e-12 && (d[5] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn colrange_maps_each_channel_to_unit_interval() {
        let mut d = vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0];
        scale_in_place(&mut d, 3, 2, Scale::ColRange);
        assert_eq!(d, vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn range_uses_the_whole_matrix() {
        let mut d = vec![0.0, 10.0, 5.0, 20.0];
        scale_in_place(&mut d, 2, 2, Scale::Range);
        assert_eq!(d, vec![0.0, 0.5, 0.25, 1.0]);
    }

    #[test]
    fn per_sample_draw_takes_at_most_n_from_each_group_and_is_seeded() {
        let groups: Vec<usize> = (0..100).map(|i| if i < 90 { 0 } else { 1 }).collect();
        let a = train_indices(100, &groups, 20, 1.0, 7).unwrap().unwrap();
        let b = train_indices(100, &groups, 20, 1.0, 7).unwrap().unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a.len(),
            30,
            "20 from the big group, all 10 of the small one"
        );
        assert_eq!(a.iter().filter(|&&i| i >= 90).count(), 10);
        assert!(a.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(complement(&a, 100).len(), 70);
    }

    #[test]
    fn prop_train_matches_r_ceiling_and_all_is_none() {
        assert!(train_indices(10, &[], 0, 1.0, 1).unwrap().is_none());
        let s = train_indices(10, &[], 0, 0.25, 1).unwrap().unwrap();
        assert_eq!(s.len(), 3);
        assert!(train_indices(10, &[], 0, 0.99, 1).unwrap().is_none());
    }

    #[test]
    fn take_rows_copies_whole_cells() {
        let d = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(take_rows(&d, 2, &[2, 0]), vec![5.0, 6.0, 1.0, 2.0]);
    }
}
