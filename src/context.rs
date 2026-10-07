//! Loading the task without the parts a transform does not use.
//!
//! `ProductionContext::from_task_id` fetches the **workflow** and extracts colour and palette
//! information from the step, and fails the whole run when the step is not in the saved
//! workflow document: `Step '…' not found in workflow`.
//!
//! That happens in normal use. Change a property in the interface and run, and the task can
//! reference a step the saved workflow does not carry yet; reset the step and run again and it
//! matches. So an operator that needs none of it — this one has no colours, no palette, no
//! chart — is failed by something it never asked for.
//!
//! Everything a transform needs is on the task itself: the `CubeQuery` (which carries the table
//! hashes and the operator settings), the schema ids, the project, and the namespace. This
//! builds the context from those and never reads the workflow.
use anyhow::{Result, anyhow, bail};
use std::sync::Arc;
use tercen_rs::TercenClient;
use tercen_rs::client::proto::{self, GetRequest, e_task};
use tercen_rs::context::{ContextBase, ContextBaseBuilder};
use tonic::Request;

/// Build a context for `task_id` from the task alone.
pub async fn from_task_id(client: Arc<TercenClient>, task_id: &str) -> Result<ContextBase> {
    let task = get_task(&client, task_id).await?;
    let (query, project_id, schema_ids, parent_task_id, environment) = match task.object.as_ref() {
        Some(e_task::Object::Runcomputationtask(t)) => (
            t.query.clone(),
            t.project_id.clone(),
            t.schema_ids.clone(),
            t.parent_task_id.clone(),
            t.environment.clone(),
        ),
        Some(e_task::Object::Computationtask(t)) => (
            t.query.clone(),
            t.project_id.clone(),
            t.schema_ids.clone(),
            t.parent_task_id.clone(),
            t.environment.clone(),
        ),
        Some(e_task::Object::Cubequerytask(t)) => (
            t.query.clone(),
            t.project_id.clone(),
            t.schema_ids.clone(),
            String::new(),
            t.environment.clone(),
        ),
        _ => bail!("task {task_id} is not a computation task"),
    };
    let query = query.ok_or_else(|| anyhow!("task {task_id} has no CubeQuery"))?;

    // Schema ids live on the task, or on the CubeQueryTask that produced it.
    let mut schema_ids = schema_ids;
    if schema_ids.is_empty() && !parent_task_id.is_empty() {
        let parent = get_task(&client, &parent_task_id).await?;
        schema_ids = match parent.object.as_ref() {
            Some(e_task::Object::Cubequerytask(t)) => t.schema_ids.clone(),
            Some(e_task::Object::Computationtask(t)) => t.schema_ids.clone(),
            Some(e_task::Object::Runcomputationtask(t)) => t.schema_ids.clone(),
            _ => bail!("parent task {parent_task_id} has an unexpected type"),
        };
    }

    let env = |key: &str| -> String {
        environment
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.value.clone())
            .unwrap_or_default()
    };
    let operator_settings = query.operator_settings.clone();
    let namespace = operator_settings
        .as_ref()
        .map(|s| s.namespace.clone())
        .unwrap_or_default();

    tracing::info!(
        task_id,
        workflow = %env("workflow.id"),
        step = %env("step.id"),
        namespace = %namespace,
        schemas = schema_ids.len(),
        "task loaded"
    );

    ContextBaseBuilder::new()
        .client(client)
        .cube_query(query)
        .schema_ids(schema_ids)
        .workflow_id(env("workflow.id"))
        .step_id(env("step.id"))
        .project_id(project_id)
        .namespace(namespace)
        .operator_settings(operator_settings)
        .build()
        .map_err(|e| anyhow!("build the context for task {task_id}: {e}"))
}

async fn get_task(client: &TercenClient, id: &str) -> Result<proto::ETask> {
    let mut svc = client
        .task_service()
        .map_err(|e| anyhow!("task service: {e}"))?;
    Ok(svc
        .get(Request::new(GetRequest {
            id: id.to_string(),
            ..Default::default()
        }))
        .await
        .map_err(|e| anyhow!("task_service.get({id}): {e}"))?
        .into_inner())
}
