//! Result upload.
//!
//! The `OperatorResult` TSON is on disk (it can be GBs); it is streamed to `FileService.upload`
//! in 1 MiB `ReqUpload` messages (the first carries the `FileDocument`).
//!
//! * **Production** (`--taskId`): the task normally already has a `fileResultId` → upload into
//!   that document; otherwise upload a new one and set `fileResultId` on the task (tercen-rs
//!   `save_table` semantics).
//! * **Dev** (`WORKFLOW_ID`/`STEP_ID`): like the Python `OperatorContextDev.save`: upload a new
//!   document, create a `RunComputationTask` with the step's `CubeQuery` and the file, run it,
//!   wait for it, and then link the step (`model.taskId`, `state`, `computedRelation`) so the
//!   result is visible in the workflow — set `DEV_NO_LINK=1` to skip the linking.
use std::path::Path;

use crate::progress::{self, Reporter};
use anyhow::{Result, anyhow, bail};
use futures::stream;
use tercen_rs::client::proto::{self, e_file_document, e_state, e_task};
use tercen_rs::context::ContextBase;
use tonic::Request;

const UPLOAD_CHUNK: usize = 1 << 20;

fn new_file_document(project_id: &str, owner: &str, size: u64) -> proto::FileDocument {
    proto::FileDocument {
        name: "result".to_string(),
        project_id: project_id.to_string(),
        size: i32::try_from(size).unwrap_or(i32::MAX),
        acl: Some(proto::Acl {
            owner: owner.to_string(),
            aces: vec![],
        }),
        metadata: Some(proto::EFileMetadata {
            object: Some(proto::e_file_metadata::Object::Filemetadata(
                proto::FileMetadata {
                    content_type: "application/octet-stream".to_string(),
                    ..Default::default()
                },
            )),
        }),
        ..Default::default()
    }
}

/// Stream `path` into `file_doc` (new when `id` is empty). Returns the FileDocument id.
pub async fn upload_file(
    ctx: &ContextBase,
    file_doc: proto::FileDocument,
    path: &Path,
    rep: &Reporter,
) -> Result<String> {
    use std::io::Read;
    let size = std::fs::metadata(path)?.len();
    let mut fs = ctx
        .client()
        .file_service()
        .map_err(|e| anyhow!("file service: {e}"))?;
    let first = Some(proto::EFileDocument {
        object: Some(e_file_document::Object::Filedocument(file_doc)),
    });
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::with_capacity(UPLOAD_CHUNK, file);
    // Lazy chunk stream: (reader, first-message flag, bytes sent, done). On a cohort-scale import
    // the upload is the longest phase by far — 149 s of a 171 s run — so it is the one that most
    // needs reporting.
    let rep = rep.clone();
    let st = stream::unfold(
        (reader, first, 0u64, false),
        move |(mut rd, mut first, mut sent, done)| {
            let rep = rep.clone();
            async move {
                if done {
                    return None;
                }
                let mut buf = vec![0u8; UPLOAD_CHUNK];
                let mut n = 0;
                while n < buf.len() {
                    match rd.read(&mut buf[n..]) {
                        Ok(0) => break,
                        Ok(k) => n += k,
                        Err(e) => {
                            tracing::error!("read result file: {e}");
                            return None;
                        }
                    }
                }
                buf.truncate(n);
                let is_first = first.is_some();
                if n == 0 && !is_first {
                    return None;
                }
                sent += n as u64;
                rep.at(
                    progress::band(progress::UPLOAD, sent as usize, size.max(1) as usize),
                    format!(
                        "Uploading the result: {} of {} MB",
                        sent / 1_000_000,
                        size / 1_000_000
                    ),
                );
                let msg = proto::ReqUpload {
                    file: first.take(),
                    bytes: buf,
                };
                Some((msg, (rd, first, sent, n < UPLOAD_CHUNK)))
            }
        },
    );
    let t0 = std::time::Instant::now();
    let resp = fs
        .upload(Request::new(st))
        .await
        .map_err(|e| anyhow!("file_service.upload: {e}"))?
        .into_inner();
    let id = match resp.result.and_then(|d| d.object) {
        Some(e_file_document::Object::Filedocument(fd)) => fd.id,
        None => bail!("upload response has no file document"),
    };
    let dt = t0.elapsed().as_secs_f64();
    tracing::info!(bytes = size, secs = format!("{dt:.1}"), mb_per_s = format!("{:.1}", size as f64 / 1e6 / dt.max(1e-9)), file_id = %id, "result uploaded");
    Ok(id)
}

/// Production: attach the result file to the running task.
pub async fn save_production(
    ctx: &ContextBase,
    task_id: &str,
    path: &Path,
    rep: &Reporter,
) -> Result<()> {
    let mut ts = ctx
        .client()
        .task_service()
        .map_err(|e| anyhow!("task service: {e}"))?;
    let mut task = ts
        .get(Request::new(proto::GetRequest {
            id: task_id.to_string(),
            ..Default::default()
        }))
        .await
        .map_err(|e| anyhow!("task_service.get({task_id}): {e}"))?
        .into_inner();
    let (existing, project_id, owner) = match task.object.as_ref() {
        Some(e_task::Object::Runcomputationtask(t)) => (
            t.file_result_id.clone(),
            t.project_id.clone(),
            t.owner.clone(),
        ),
        Some(e_task::Object::Computationtask(t)) => (
            t.file_result_id.clone(),
            t.project_id.clone(),
            t.owner.clone(),
        ),
        _ => bail!("task {task_id} is not a (Run)ComputationTask"),
    };
    let size = std::fs::metadata(path)?.len();
    if existing.is_empty() {
        let id = upload_file(ctx, new_file_document(&project_id, &owner, size), path, rep).await?;
        match task.object.as_mut() {
            Some(e_task::Object::Runcomputationtask(t)) => t.file_result_id = id,
            Some(e_task::Object::Computationtask(t)) => t.file_result_id = id,
            _ => unreachable!(),
        }
        ts.update(Request::new(task))
            .await
            .map_err(|e| anyhow!("task_service.update: {e}"))?;
    } else {
        let mut fs = ctx
            .client()
            .file_service()
            .map_err(|e| anyhow!("file service: {e}"))?;
        let doc = fs
            .get(Request::new(proto::GetRequest {
                id: existing.clone(),
                ..Default::default()
            }))
            .await
            .map_err(|e| anyhow!("file_service.get({existing}): {e}"))?
            .into_inner();
        let fd = match doc.object {
            Some(e_file_document::Object::Filedocument(fd)) => fd,
            None => bail!("EFileDocument without object"),
        };
        upload_file(ctx, fd, path, rep).await?;
    }
    Ok(())
}

pub struct DevSaved {
    pub task_id: String,
    pub file_id: String,
    pub computed_relation: Option<proto::ERelation>,
}

/// Dev: upload, create + run + await a RunComputationTask on the step's CubeQuery, link the step.
pub async fn save_dev(
    ctx: &ContextBase,
    workflow_id: &str,
    step_id: &str,
    path: &Path,
) -> Result<DevSaved> {
    use proto::{e_step, e_workflow};
    let mut ws = ctx
        .client()
        .workflow_service()
        .map_err(|e| anyhow!("workflow service: {e}"))?;
    let e_wf = ws
        .get(Request::new(proto::GetRequest {
            id: workflow_id.to_string(),
            ..Default::default()
        }))
        .await
        .map_err(|e| anyhow!("workflow_service.get({workflow_id}): {e}"))?
        .into_inner();
    let mut wf = match e_wf.object {
        Some(e_workflow::Object::Workflow(w)) => w,
        None => bail!("EWorkflow without object"),
    };
    let project_id = wf.project_id.clone();
    let owner = wf.acl.as_ref().map(|a| a.owner.clone()).unwrap_or_default();
    let size = std::fs::metadata(path)?.len();
    let file_id = upload_file(
        ctx,
        new_file_document(&project_id, &owner, size),
        path,
        &Reporter::silent(),
    )
    .await?;

    let task = proto::ETask {
        object: Some(e_task::Object::Runcomputationtask(
            proto::RunComputationTask {
                state: Some(proto::EState {
                    object: Some(e_state::Object::Initstate(proto::InitState::default())),
                }),
                owner: owner.clone(),
                project_id: project_id.clone(),
                query: Some(ctx.cube_query().clone()),
                file_result_id: file_id.clone(),
                ..Default::default()
            },
        )),
    };
    let mut ts = ctx
        .client()
        .task_service()
        .map_err(|e| anyhow!("task service: {e}"))?;
    let created = ts
        .create(Request::new(task))
        .await
        .map_err(|e| anyhow!("task_service.create: {e}"))?
        .into_inner();
    let task_id = match created.object.as_ref() {
        Some(e_task::Object::Runcomputationtask(t)) => t.id.clone(),
        _ => bail!("created task has unexpected type"),
    };
    tracing::info!(task_id, "task created; running");
    ts.run_task(Request::new(proto::ReqRunTask {
        task_id: task_id.clone(),
    }))
    .await
    .map_err(|e| anyhow!("task_service.runTask: {e}"))?;
    let done = ts
        .wait_done(Request::new(proto::ReqWaitDone {
            task_id: task_id.clone(),
        }))
        .await
        .map_err(|e| anyhow!("task_service.waitDone: {e}"))?
        .into_inner();
    let done_task = done
        .result
        .ok_or_else(|| anyhow!("waitDone returned no task"))?;
    let (state, computed_relation) = match done_task.object.as_ref() {
        Some(e_task::Object::Runcomputationtask(t)) => {
            (t.state.clone(), t.computed_relation.clone())
        }
        Some(e_task::Object::Computationtask(t)) => (t.state.clone(), t.computed_relation.clone()),
        _ => bail!("waitDone returned an unexpected task type"),
    };
    match state.as_ref().and_then(|s| s.object.as_ref()) {
        Some(e_state::Object::Donestate(_)) => tracing::info!(task_id, "task done"),
        Some(e_state::Object::Failedstate(f)) => {
            bail!("task {task_id} failed: {} {}", f.error, f.reason)
        }
        other => bail!(
            "task {task_id} ended in unexpected state {:?}",
            other.map(std::mem::discriminant)
        ),
    }

    if std::env::var("DEV_NO_LINK").is_err() {
        // Link the step to this task so the result shows in the workflow (what the server does
        // for a normal run).
        let mut linked = false;
        for st in wf.steps.iter_mut() {
            if let Some(e_step::Object::Datastep(ds)) = st.object.as_mut()
                && ds.id == step_id
            {
                ds.state = Some(proto::StepState {
                    task_id: task_id.clone(),
                    task_state: Some(proto::EState {
                        object: Some(e_state::Object::Donestate(proto::DoneState::default())),
                    }),
                });
                ds.computed_relation = computed_relation.clone();
                if let Some(m) = ds.model.as_mut() {
                    m.task_id = task_id.clone();
                }
                linked = true;
            }
        }
        if linked {
            let e_wf = proto::EWorkflow {
                object: Some(e_workflow::Object::Workflow(wf)),
            };
            ws.update(Request::new(e_wf))
                .await
                .map_err(|e| anyhow!("workflow_service.update: {e}"))?;
            tracing::info!(
                step_id,
                "step linked to the dev task (computedRelation set)"
            );
        } else {
            tracing::warn!(
                step_id,
                "DataStep not found in workflow — result not linked"
            );
        }
    }
    Ok(DevSaved {
        task_id,
        file_id,
        computed_relation,
    })
}
