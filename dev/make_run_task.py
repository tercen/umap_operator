"""Create a RunComputationTask the way the platform does, so the *production* binary can be
exercised without publishing anything.

The platform creates this task with the step's CubeQuery, pre-allocates an empty result file and
puts its id in `fileResultId`, then runs the operator with --taskId/--serviceUri/--token. The
operator is expected to upload into that existing file. Dev mode never takes that branch, so it
is the last untested piece of the production path.

usage: make_run_task.py <workflow id> <data step id> [--with-file-result]
"""
import os, sys
import tercen.model.impl as m
from tercen.client.factory import TercenClient

tok = os.environ["TERCEN_TOKEN"]
wf_id, step_id = sys.argv[1], sys.argv[2]
prealloc = "--with-file-result" in sys.argv

c = TercenClient("http://127.0.0.1:5402")
c.userService.tercenClient.token = tok
c.httpClient.authorization = tok

wf = c.workflowService.get(wf_id)
ds = next(s for s in wf.steps if s.id == step_id)
cq = c.taskService.get(ds.model.taskId).query      # the step's done CubeQueryTask

task = m.RunComputationTask()
task.state = m.InitState()
task.owner = wf.acl.owner
task.projectId = wf.projectId
task.query = cq
if prealloc:
    fd = m.FileDocument()
    fd.name = "result"
    fd.projectId = wf.projectId
    fd.acl.owner = wf.acl.owner
    fd.metadata.contentType = "application/octet-stream"
    fd = c.fileService.upload(fd, b"")
    task.fileResultId = fd.id
task = c.taskService.create(task)
print("TASK", task.id)
print("FILE_RESULT", task.fileResultId or "(none)")
