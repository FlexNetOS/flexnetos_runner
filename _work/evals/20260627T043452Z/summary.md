# FlexNetOS Runner Evaluation — 20260627T043452Z

- repo: `FlexNetOS/flexnetos_runner`
- workflow: `runner-smoke.yml`
- ref: `main`
- isolate peers: `1`
- started: `2026-06-27T04:34:52Z`

## Live Results

| Slot | Runner | Conclusion | Accuracy | Dispatch→visible | Dispatch→created | Pickup latency | Exec time | Total | Run |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| 01 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-01` | success | pass | 1.355s | 0.355s | 4.000s | 2.000s | 6.355s | [28278756792](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28278756792) |
| 02 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-02` | success | pass | 1.222s | 0.474s | 3.000s | 3.000s | 6.474s | [28278785872](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28278785872) |

## Task Output Observed

### fxrun-drdave-TRX50-AI-TOP-flexnetos-01

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_bd14f8cb-69f5-4dd2-a222-ddbfcdc39699","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-27T04:36:07Z"}
```

### fxrun-drdave-TRX50-AI-TOP-flexnetos-02

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_f28cd0aa-d453-40c5-9eac-b92cb99a2bba","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-27T04:37:32Z"}
```

## Failures

No step failures recorded.

## Final GitHub Runner API Snapshot

```json
[{"busy":false,"id":4730,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","status":"online"},{"busy":false,"id":4731,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","status":"online"}]
```

## Metrics JSONL

```json
{"slot":"01","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-01.service","run_id":"28278756792","job_id":"83790411304","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28278756792","dispatch_iso":"2026-06-27T04:36:02Z","created_at":"2026-06-27T04:36:03Z","updated_at":"2026-06-27T04:36:10Z","job_started":"2026-06-27T04:36:07Z","job_completed":"2026-06-27T04:36:09Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-01","timings_ms":{"dispatch_to_visible_ms":1355,"dispatch_to_created_ms":355,"pickup_latency_ms":4000,"exec_ms":2000,"total_ms":6355},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_bd14f8cb-69f5-4dd2-a222-ddbfcdc39699","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-27T04:36:07Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:36:07Z","completedAt":"2026-06-27T04:36:07Z","durationMs":0},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:36:07Z","completedAt":"2026-06-27T04:36:07Z","durationMs":0},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:36:07Z","completedAt":"2026-06-27T04:36:08Z","durationMs":1000}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is below 10s","end-to-end turnaround is below 60s"]}
{"slot":"02","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service","run_id":"28278785872","job_id":"83790493300","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28278785872","dispatch_iso":"2026-06-27T04:37:27Z","created_at":"2026-06-27T04:37:28Z","updated_at":"2026-06-27T04:37:34Z","job_started":"2026-06-27T04:37:31Z","job_completed":"2026-06-27T04:37:34Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-02","timings_ms":{"dispatch_to_visible_ms":1222,"dispatch_to_created_ms":474,"pickup_latency_ms":3000,"exec_ms":3000,"total_ms":6474},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_f28cd0aa-d453-40c5-9eac-b92cb99a2bba","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-27T04:37:32Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:37:31Z","completedAt":"2026-06-27T04:37:31Z","durationMs":0},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:37:31Z","completedAt":"2026-06-27T04:37:32Z","durationMs":1000},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-27T04:37:32Z","completedAt":"2026-06-27T04:37:32Z","durationMs":0}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is below 10s","end-to-end turnaround is below 60s"]}
```

## Lessons Learned

- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] identity and repo-local workspace assertions passed
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] workflow completed successfully
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] runner pickup latency is below 10s
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] end-to-end turnaround is below 60s
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] identity and repo-local workspace assertions passed
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] workflow completed successfully
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] runner pickup latency is below 10s
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] end-to-end turnaround is below 60s

## Artifact Directory

`_work/evals/20260627T043452Z`
