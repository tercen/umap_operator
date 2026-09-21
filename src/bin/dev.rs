//! Dev binary: runs against a Studio workflow step (`--workflowId` / `--stepId`), so the
//! operator can be exercised without publishing anything (create-rust-operator §8a).
use anyhow::Result;

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
use umap_operator::{init_tracing, require_env, run_dev};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--workflowId" => {
                unsafe { std::env::set_var("WORKFLOW_ID", &args[i + 1]) };
                i += 2;
            }
            "--stepId" => {
                unsafe { std::env::set_var("STEP_ID", &args[i + 1]) };
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
    run_dev(&require_env("WORKFLOW_ID")?, &require_env("STEP_ID")?).await
}
