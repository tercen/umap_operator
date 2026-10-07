//! Production binary: Tercen invokes `--taskId X --serviceUri Y --token Z`.
use anyhow::Result;

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
use umap_operator::{init_tracing, require_env, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--taskId" => {
                unsafe { std::env::set_var("TERCEN_TASK_ID", &args[i + 1]) };
                i += 2;
            }
            "--serviceUri" => {
                unsafe { std::env::set_var("TERCEN_URI", &args[i + 1]) };
                i += 2;
            }
            "--token" => {
                unsafe { std::env::set_var("TERCEN_TOKEN", &args[i + 1]) };
                i += 2;
            }
            _ => i += 1,
        }
    }
    let task_id = require_env("TERCEN_TASK_ID")?;
    run(&task_id).await
}
