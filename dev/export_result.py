"""Export the result table of a dev step, for the platform test's golden.

    . dev/studio.env; python dev/export_result.py <workflowId> <stepId> <out.csv>

Walks the step's computedRelation for the SimpleRelation whose schema carries this operator's
columns (`*.umap.1`), then exports it with tercenctl — so the golden is what the *server* read
from the bytes the operator wrote, not the in-process result.
"""
import os, subprocess, sys
import tercen.model.impl as m
from tercen.client.factory import TercenClient

wf_id, step_id, out = sys.argv[1:4]
c = TercenClient(os.environ.get("TERCEN_HTTP", "http://127.0.0.1:5402"))
tok = os.environ["TERCEN_TOKEN"]
c.userService.tercenClient.token = tok; c.httpClient.authorization = tok
wf = c.workflowService.get(wf_id)
step = next(s for s in wf.steps if s.id == step_id)

def simple_ids(rel, acc):
    if rel is None:
        return acc
    if isinstance(rel, m.SimpleRelation):
        acc.append(rel.id)
    for attr in ("relation", "mainRelation", "leftRelation", "rightRelation"):
        if hasattr(rel, attr):
            simple_ids(getattr(rel, attr), acc)
    for attr in ("joinOperators",):
        for j in getattr(rel, attr, []) or []:
            simple_ids(getattr(j, "rightRelation", None), acc)
    return acc

ids = simple_ids(step.computedRelation, [])
hit = None
for sid in ids:
    try:
        sch = c.tableSchemaService.get(sid)
    except Exception:
        continue
    names = [col.name for col in sch.columns]
    if any(n.endswith(".umap.1") for n in names):
        hit = sid; print("result schema", sid, names, "rows", sch.nRows); break
if not hit:
    sys.exit(f"no schema with umap.1 among {ids}")
subprocess.check_call(["tercenctl", "--context", "studio", "data", "export-csv", "-s", hit, "--filePath", out])
print("exported", out)
