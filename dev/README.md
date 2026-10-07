# Dev loop (local Studio only)

- `studio.env` (git-ignored): `TERCEN_URI` = gRPC of the Studio `tercen` container (`docker inspect` IP, port 50051), `TERCEN_HTTP`, `TERCEN_TOKEN` (`userService.connect("admin","admin")`).
- `studio_ids.env` (git-ignored): project, uploaded table schema, workflow and step ids on this machine. Recreate with:
  ```
  tercenctl --context studio project create -n "umap_rust_operator dev"
  tercenctl --context studio data upload-csv -p <project> --filePath tests/umap_input.csv -n umap_input
  python dev/setup_umap.py <workflowId> <schemaId> train_cells_per_sample=500 sample_factor=sample min_dist=0.01 seed=42 threads=1
  ```
  (`tercenctl workflow create` did not find the project by id; the Python client's `workflowService.create` does.)
- Run: `. dev/studio.env; target/release/dev --workflowId … --stepId …`, under `/usr/bin/time -v` with the worker's `MALLOC_CONF` for the memory model.
- `export_result.py <wf> <step> <out.csv>` exports what the **server** read back — the golden in `tests/` comes from it, and running twice and `cmp`-ing the exports is the reproducibility check.
