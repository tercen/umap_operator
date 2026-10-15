//! Operator properties.
//!
//! Names follow the R `umap_operator` (`init`, `scale`, `n_neighbors`, `spread`, `min_dist`,
//! `pca`, `prop.train`, `seed`) so a workflow can swap one step for the other, plus three this
//! operator adds: `train_cells_per_sample` and `sample_factor`, which express "5,000 cells per
//! file" where `prop.train` cannot, and `threads`.
use anyhow::{Result, bail};
use tercen_rs::PropertyReader;
use tercen_rs::context::ContextBase;
use umaprs::InitMethod;

/// `scale`, with uwot's meaning for each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    None,
    /// Each channel centred and divided by its standard deviation.
    Z,
    /// Each channel centred, then the whole matrix divided by its largest absolute value.
    MaxAbs,
    /// The whole matrix mapped to [0, 1].
    Range,
    /// Each channel mapped to [0, 1].
    ColRange,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub init: InitMethod,
    pub scale: Scale,
    pub n_neighbors: usize,
    pub spread: f64,
    pub min_dist: f64,
    /// `pca`: reduce to this many components first. `None` when the property is ≤ 0.
    pub pca: Option<usize>,
    /// `prop.train`: fraction of cells to fit on; the rest are projected. 1 fits on everything.
    pub prop_train: f64,
    /// `train_cells_per_sample`: 0 is off. When set it replaces `prop.train`: this many cells
    /// are drawn from every sample, so a small file is represented as well as a large one.
    pub train_cells_per_sample: usize,
    /// `sample_factor`: the column factor that names the sample. Empty takes the first column
    /// factor — unless that factor has one value per cell, which is the event id, not a sample.
    pub sample_factor: String,
    /// `seed`. Always set; a negative value is refused rather than read as "random".
    pub seed: u64,
    /// `n_epochs`: 0 lets the library choose (500 below 10,000 cells, 200 above, as umap-learn).
    pub n_epochs: usize,
    /// `threads`: 0 is every core the container is given. 1 is bit-for-bit reproducible.
    pub threads: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            init: InitMethod::Auto,
            scale: Scale::None,
            n_neighbors: 15,
            spread: 1.0,
            min_dist: 0.5,
            pca: None,
            prop_train: 1.0,
            train_cells_per_sample: 0,
            sample_factor: String::new(),
            seed: 42,
            n_epochs: 0,
            threads: 0,
        }
    }
}

/// The R operator (1.x) defaults `seed` to -1, read as "random", so every step upgraded from 1.x
/// carries -1. Any fixed seed is one possible random draw, so a negative seed takes the default
/// rather than failing the step, and the result can still be repeated.
fn seed_from(v: f64, default: u64) -> u64 {
    if v < 0.0 {
        tracing::warn!(
            "property 'seed' is {v} (the R operator's 'random'); using {default} so the step \
             can be repeated"
        );
        return default;
    }
    v as u64
}

/// Read the properties off the task's `CubeQueryTask` snapshot.
pub fn read(ctx: &ContextBase) -> Result<Settings> {
    let pr = PropertyReader::from_operator_settings(ctx.operator_settings());
    let d = Settings::default();
    // Every number is parsed as f64: the property panel stores "15.0" for a DoubleProperty
    // whatever the operator wants, and an integer parse of that fails silently to the default.
    let num = |name: &str, dflt: f64| -> Result<f64> {
        let raw = pr.get_string(name, "").trim().to_string();
        if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
            return Ok(dflt);
        }
        raw.parse::<f64>()
            .map_err(|_| anyhow::anyhow!("property '{name}' should be a number, got '{raw}'"))
    };
    let text = |name: &str| pr.get_string(name, "").trim().to_string();

    let init = match text("init").to_lowercase().as_str() {
        "" | "auto" => InitMethod::Auto,
        // uwot's spectral family and its variants all become the one spectral start here.
        "spectral" | "normlaplacian" | "laplacian" | "agspectral" => InitMethod::Spectral,
        "pca" | "spca" => InitMethod::Pca,
        "random" | "lvrandom" => InitMethod::Random,
        other => bail!(
            "property 'init' is '{other}'; use auto, spectral, pca or random (uwot's \
             normlaplacian/laplacian/agspectral map to spectral, spca to pca, lvrandom to random)"
        ),
    };
    let scale = match text("scale").to_lowercase().as_str() {
        "" | "none" | "false" => Scale::None,
        "z" | "scale" | "true" => Scale::Z,
        "maxabs" => Scale::MaxAbs,
        "range" => Scale::Range,
        "colrange" => Scale::ColRange,
        other => bail!("property 'scale' is '{other}'; use none, Z, maxabs, range or colrange"),
    };
    let n_neighbors = num("n_neighbors", d.n_neighbors as f64)?;
    if n_neighbors < 2.0 {
        bail!("property 'n_neighbors' must be at least 2, got {n_neighbors}");
    }
    let spread = num("spread", d.spread)?;
    let min_dist = num("min_dist", d.min_dist)?;
    if spread.is_nan() || min_dist.is_nan() || spread <= 0.0 || min_dist < 0.0 || min_dist > spread
    {
        bail!(
            "properties 'spread' ({spread}) and 'min_dist' ({min_dist}) must satisfy \
             0 <= min_dist <= spread, spread > 0"
        );
    }
    let pca = match num("pca", -1.0)? {
        v if v <= 0.0 => None,
        v => Some(v as usize),
    };
    let prop_train = num("prop.train", d.prop_train)?;
    if prop_train.is_nan() || prop_train <= 0.0 || prop_train > 1.0 {
        bail!("property 'prop.train' must be between 0 and 1, got {prop_train}");
    }
    let train_cells_per_sample = num("train_cells_per_sample", 0.0)?.max(0.0) as usize;
    let seed = seed_from(num("seed", d.seed as f64)?, d.seed);
    let s = Settings {
        init,
        scale,
        n_neighbors: n_neighbors as usize,
        spread,
        min_dist,
        pca,
        prop_train,
        train_cells_per_sample,
        sample_factor: text("sample_factor"),
        seed,
        n_epochs: num("n_epochs", 0.0)?.max(0.0) as usize,
        threads: num("threads", 0.0)?.max(0.0) as usize,
    };
    tracing::info!(?s, "properties");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn manifest() -> Value {
        serde_json::from_str(include_str!("../operator.json")).unwrap()
    }

    /// Every property the reader looks at is declared, with the same default; the manifest's
    /// `kind`s come from the platform's closed vocabulary.
    /// The platform books `intercept × n_cols + 1.5 × offset` MB, and `n_cols` is the number of
    /// cells (the column table). A full fit costs about 2.1 KB per cell at 8 channels and 2.6 KB
    /// at 40 (`examples/fit_memory.rs`); 2.0.0 booked by value and so gave 8-channel data a
    /// third of what it needed (695,052 cells: 756 MB booked, killed at 794 MB).
    #[test]
    fn memory_model_books_a_full_fit_by_cells() {
        let m: Value = serde_json::from_str(include_str!("../memory_model.json")).unwrap();
        assert_eq!(m["features"][0]["name"], "n_cols");
        let book = |cells: f64| {
            m["intercept"].as_f64().unwrap() * cells + 1.5 * m["offset"].as_f64().unwrap()
        };
        // Measured fit peaks (MB), with 20% headroom on top for reading and writing.
        for (cells, measured) in [(100_000.0, 210.0), (300_000.0, 607.0), (695_052.0, 1435.0)] {
            assert!(
                book(cells) >= 1.2 * measured,
                "{cells} cells book {}",
                book(cells)
            );
        }
        assert!(book(200_000.0) >= 1.2 * 517.0, "40 channels");
    }

    #[test]
    fn a_negative_seed_from_1x_takes_the_default() {
        assert_eq!(seed_from(-1.0, 42), 42);
        assert_eq!(seed_from(-7.0, 42), 42);
        assert_eq!(seed_from(0.0, 42), 0);
        assert_eq!(seed_from(123.0, 42), 123);
    }

    #[test]
    fn manifest_declares_what_the_code_reads() {
        let m = manifest();
        let props: Vec<&Value> = m["properties"].as_array().unwrap().iter().collect();
        let by_name = |n: &str| {
            props
                .iter()
                .find(|p| p["name"] == n)
                .unwrap_or_else(|| panic!("property '{n}' is not declared in operator.json"))
        };
        let d = Settings::default();
        assert_eq!(by_name("init")["defaultValue"], "auto");
        assert_eq!(by_name("scale")["defaultValue"], "none");
        assert_eq!(by_name("n_neighbors")["defaultValue"], d.n_neighbors as f64);
        assert_eq!(by_name("spread")["defaultValue"], d.spread);
        assert_eq!(by_name("min_dist")["defaultValue"], d.min_dist);
        assert_eq!(by_name("pca")["defaultValue"], -1.0);
        assert_eq!(by_name("prop.train")["defaultValue"], d.prop_train);
        assert_eq!(by_name("train_cells_per_sample")["defaultValue"], 0.0);
        assert_eq!(by_name("sample_factor")["defaultValue"], "");
        assert_eq!(by_name("seed")["defaultValue"], d.seed as f64);
        assert_eq!(by_name("n_epochs")["defaultValue"], 0.0);
        assert_eq!(by_name("threads")["defaultValue"], 0.0);
        let init_values: Vec<&str> = by_name("init")["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(init_values, ["auto", "spectral", "pca", "random"]);

        let allowed = [
            "OperatorSpec",
            "CrosstabSpec",
            "MetaFactor",
            "AxisSpec",
            "OperatorJoinSpec",
            "JoinOperator",
            "ColumnPair",
            "TableRelation",
            "Attribute",
            "Pair",
        ];
        fn walk(v: &Value, allowed: &[&str]) {
            match v {
                Value::Object(o) => {
                    if let Some(Value::String(k)) = o.get("kind") {
                        assert!(allowed.contains(&k.as_str()), "invented kind '{k}'");
                    }
                    o.values().for_each(|c| walk(c, allowed));
                }
                Value::Array(a) => a.iter().for_each(|c| walk(c, allowed)),
                _ => {}
            }
        }
        walk(&m["operatorSpec"], &allowed);
        let attrs: Vec<&str> = m["operatorSpec"]["outputSpecsV2"][0]["joinOperators"][0]
            ["rightRelation"]["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(attrs, ["umap.1", "umap.2"]);
        assert!(
            {
                let c = m["container"].as_str().unwrap();
                let tag = c.rsplit_once(':').map(|(_, t)| t).unwrap_or("");
                tag.split('.').count() == 3
                    && tag
                        .split('.')
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            },
            "container must pin an exact x.y.z version tag"
        );
    }
}
