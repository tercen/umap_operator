//! umap_operator — UMAP for Tercen, as a drop-in for the R `umap_operator`.
//!
//! Rows are channels, columns are cells, y is the value. The result is one row per cell with
//! its two embedding coordinates. The library is `umaprs` (tercen/umaprs, parity branch):
//! umap-learn's graph and transform, checked stage by stage against it.
//!
//! **The shape of the problem is a transpose**, as for FlowSOM: the crosstab arrives as
//! scattered `(.ri, .ci, .y)` and UMAP needs each cell's vector across channels. The operator
//! gathers the matrix as `n_cells × p` of `f64` and the memory model declares that cost.
//!
//! **Fit on a draw, project the rest.** With `train_cells_per_sample` (or `prop.train`) the
//! model is fitted on a seeded draw and every other cell is placed by `transform`, the way
//! umap-learn and uwot do it. That is what makes a cohort of millions of cells a few minutes
//! rather than an hour, and what makes two runs on different files comparable.
pub mod algorithm;
pub mod context;
pub mod input;
pub mod output;
pub mod pagecache;
pub mod progress;
pub mod props;
pub mod tson;
pub mod upload;

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use ndarray::Array2;
use tercen_rs::context::ContextBase;
use tercen_rs::{DevContext, TercenClient};

use progress::Reporter;
use tson::TsonWriter;

const CHUNK: usize = 1_000_000;

pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

pub fn require_env(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| anyhow!("{name} is not set"))
}

pub async fn run(task_id: &str) -> Result<()> {
    tracing::info!("umap_operator starting (task_id={task_id})");
    let client = build_client().await?;
    let ctx = context::from_task_id(client, task_id).await?;
    execute(
        &ctx,
        Mode::Production {
            task_id: task_id.to_string(),
        },
    )
    .await
}

pub async fn run_dev(workflow_id: &str, step_id: &str) -> Result<()> {
    tracing::info!("umap_operator starting in dev mode ({workflow_id} / {step_id})");
    let client = build_client().await?;
    let ctx = DevContext::from_workflow_step(client, workflow_id, step_id)
        .await
        .map_err(|e| anyhow!("load workflow {workflow_id} / step {step_id}: {e}"))?;
    execute(
        &ctx,
        Mode::Dev {
            workflow_id: workflow_id.to_string(),
            step_id: step_id.to_string(),
        },
    )
    .await
}

enum Mode {
    Production {
        task_id: String,
    },
    Dev {
        workflow_id: String,
        step_id: String,
    },
}

async fn build_client() -> Result<Arc<TercenClient>> {
    let client = TercenClient::from_env()
        .await
        .map_err(|e| anyhow!("connect to Tercen: {e}"))?;
    tracing::info!("connected to Tercen");
    Ok(Arc::new(client))
}

async fn execute(ctx: &ContextBase, mode: Mode) -> Result<()> {
    let t_start = Instant::now();
    tracing::info!(
        workflow = ctx.workflow_id(),
        step = ctx.step_id(),
        namespace = ctx.namespace(),
        "context loaded"
    );
    let rep = match &mode {
        Mode::Production { task_id } => Reporter::spawn(Arc::clone(ctx.client()), task_id.clone()),
        Mode::Dev { .. } => Reporter::silent(),
    };
    let s = props::read(ctx)?;

    let n_values = input::cell_count(ctx).await?;
    let channels = input::row_labels(ctx).await?;
    let p = channels.len();
    if p == 0 {
        anyhow::bail!("the projection has no rows: put the channels on rows");
    }
    let n_cells = n_values / p;
    tracing::info!(n_values, n_cells, channels = p, "projection");
    if n_cells <= s.n_neighbors {
        anyhow::bail!(
            "the projection has {n_cells} cells and n_neighbors is {}; UMAP needs more cells \
             than neighbours",
            s.n_neighbors
        );
    }

    // The sample each cell belongs to, for `train_cells_per_sample`. A column factor with one
    // value per cell is the event id, not a sample: treat it as one sample and say so.
    let groups = if s.train_cells_per_sample > 0 {
        let g = input::column_groups(ctx, &s.sample_factor).await?;
        let n_groups = g.iter().copied().max().map_or(0, |m| m + 1);
        if g.is_empty() || n_groups == n_cells {
            rep.info(format!(
                "train_cells_per_sample: the column factor has one value per cell, so all \
                 {n_cells} cells count as one sample. Set sample_factor to the file or sample \
                 factor to draw per sample."
            ));
            vec![0; n_cells]
        } else {
            tracing::info!(n_groups, "samples");
            g
        }
    } else {
        Vec::new()
    };

    rep.at(0, "Reading the crosstab");
    let mut data = gather(ctx, n_values, n_cells, p, &rep).await?;
    algorithm::scale_in_place(&mut data, n_cells, p, s.scale);

    let train = algorithm::train_indices(
        n_cells,
        &groups,
        s.train_cells_per_sample,
        s.prop_train,
        s.seed,
    )?;
    if train.is_some() && s.pca.is_some() {
        anyhow::bail!(
            "pca cannot be combined with a training draw yet: the projection is not stored \
             on the model, so the remaining cells could not be transformed. Set pca to -1, \
             or fit on every cell."
        );
    }

    let mut umap = umaprs::UMAP::new()
        .n_neighbors(s.n_neighbors)
        .min_dist(s.min_dist)
        .spread(s.spread)
        .init(s.init)
        .random_state(s.seed)
        .threads(s.threads);
    if s.n_epochs > 0 {
        umap = umap.n_epochs(s.n_epochs);
    }
    if let Some(k) = s.pca {
        umap = umap.pca(k);
    }

    rep.at(progress::FIT.0, "Fitting UMAP");
    let t = Instant::now();
    let (x, y) = match train {
        None => {
            let m = Array2::from_shape_vec((n_cells, p), data)
                .map_err(|e| anyhow!("shape the matrix: {e}"))?;
            let emb = umap.fit_transform(&m);
            drop(m);
            rep.info(format!(
                "UMAP: {n_cells} cells x {p} channels, n_neighbors {}, min_dist {}, seed {}",
                s.n_neighbors, s.min_dist, s.seed
            ));
            split_xy(&emb)
        }
        Some(idx) => {
            let rest = algorithm::complement(&idx, n_cells);
            let train_m =
                Array2::from_shape_vec((idx.len(), p), algorithm::take_rows(&data, p, &idx))
                    .map_err(|e| anyhow!("shape the training matrix: {e}"))?;
            let (emb_train, model) = umap.fit(&train_m);
            drop(train_m);
            let t_fit = t.elapsed().as_secs_f64();
            rep.at(
                progress::band(progress::FIT, 1, 2),
                format!("Projecting {} cells onto the model", rest.len()),
            );
            let rest_m =
                Array2::from_shape_vec((rest.len(), p), algorithm::take_rows(&data, p, &rest))
                    .map_err(|e| anyhow!("shape the projection matrix: {e}"))?;
            drop(data);
            let emb_rest = model.transform(&rest_m);
            drop(rest_m);
            rep.info(format!(
                "UMAP: fitted on {} of {n_cells} cells ({p} channels, {} samples) in {t_fit:.0} s, \
                 projected the other {} in {:.0} s; n_neighbors {}, min_dist {}, seed {}",
                idx.len(),
                groups.iter().copied().max().map_or(1, |m| m + 1),
                rest.len(),
                t.elapsed().as_secs_f64() - t_fit,
                s.n_neighbors,
                s.min_dist,
                s.seed
            ));
            let mut x = vec![0.0; n_cells];
            let mut y = vec![0.0; n_cells];
            for (k, &i) in idx.iter().enumerate() {
                x[i] = emb_train[[k, 0]];
                y[i] = emb_train[[k, 1]];
            }
            for (k, &i) in rest.iter().enumerate() {
                x[i] = emb_rest[[k, 0]];
                y[i] = emb_rest[[k, 1]];
            }
            (x, y)
        }
    };
    tracing::info!(
        secs = format!("{:.1}", t.elapsed().as_secs_f64()),
        "embedding done"
    );

    rep.at(progress::WRITE.0, "Writing the result");
    let work_root =
        std::env::temp_dir().join(format!("umap_op_{}_{}", ctx.workflow_id(), ctx.step_id()));
    std::fs::create_dir_all(&work_root)
        .with_context(|| format!("create {}", work_root.display()))?;
    let _guard = TempDirGuard(work_root.clone());
    let result_path = work_root.join("result.tson");
    {
        let f = std::fs::File::create(&result_path)
            .with_context(|| format!("create {}", result_path.display()))?;
        let w = std::io::BufWriter::with_capacity(4 << 20, pagecache::Releasing::new(f, 256 << 20));
        let mut w = TsonWriter::new(w)?;
        output::write_embedding(&mut w, &table_name(ctx), ctx.namespace(), &x, &y)?;
        output::write_footer(&mut w)?;
    }
    let bytes = std::fs::metadata(&result_path)?.len();
    tracing::info!(bytes, "result written");
    pagecache::release_path(&result_path);

    rep.at(progress::UPLOAD.0, "Uploading the result");
    match mode {
        Mode::Production { task_id } => {
            upload::save_production(ctx, &task_id, &result_path, &rep).await?
        }
        Mode::Dev {
            workflow_id,
            step_id,
        } => {
            let saved = upload::save_dev(ctx, &workflow_id, &step_id, &result_path).await?;
            tracing::info!(task_id = saved.task_id, "dev result saved");
        }
    }
    rep.at(100, "Done");
    tracing::info!(
        total_secs = format!("{:.1}", t_start.elapsed().as_secs_f64()),
        peak_rss_kb = peak_rss_kb().unwrap_or(0),
        "done"
    );
    Ok(())
}

fn split_xy(emb: &Array2<f64>) -> (Vec<f64>, Vec<f64>) {
    (emb.column(0).to_vec(), emb.column(1).to_vec())
}

/// Gather the crosstab into a cell-by-channel matrix (cell-major). Scattered by `(.ri, .ci)`,
/// so no ordering is assumed.
async fn gather(
    ctx: &ContextBase,
    n_values: usize,
    n_cells: usize,
    p: usize,
    rep: &Reporter,
) -> Result<Vec<f64>> {
    let mut data = vec![f64::NAN; n_cells * p];
    let mut seen = 0usize;
    let mut out_of_range = 0usize;
    input::for_each_chunk(ctx, &[".ri", ".ci", ".y"], n_values, CHUNK, |c| {
        for k in 0..c.y.len() {
            let (ri, ci) = (c.ri[k] as usize, c.ci[k] as usize);
            if ri >= p || ci >= n_cells {
                out_of_range += 1;
                continue;
            }
            data[ci * p + ri] = c.y[k];
        }
        seen += c.len();
        rep.at(
            progress::band(progress::READ, seen, n_values.max(1)),
            format!("Read {seen} of {n_values}"),
        );
        Ok(())
    })
    .await?;
    if out_of_range > 0 {
        anyhow::bail!(
            "{out_of_range} values fell outside the {n_cells} x {p} crosstab the schema \
             described; the projection changed under the run"
        );
    }
    let missing = data.iter().filter(|v| !v.is_finite()).count();
    if missing > 0 {
        anyhow::bail!(
            "{missing} of {} cell-channel pairs are missing or not finite. UMAP needs a complete \
             matrix: every cell must have every channel. Filter the projection, or fill the gaps \
             upstream.",
            data.len()
        );
    }
    Ok(data)
}

struct TempDirGuard(std::path::PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn table_name(ctx: &ContextBase) -> String {
    format!("{}_{}", ctx.step_id(), ctx.qt_hash())
}

fn peak_rss_kb() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
}
