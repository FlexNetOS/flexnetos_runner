# FlexNetOS Runner Evaluation — 20260626T224513Z

- repo: `FlexNetOS/flexnetos_runner`
- workflow: `runner-smoke.yml`
- ref: `main`
- isolate peers: `1`
- started: `2026-06-26T22:45:13Z`

## Live Results

| Slot | Runner | Conclusion | Accuracy | Dispatch→visible | Dispatch→created | Pickup latency | Exec time | Total | Run |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| 01 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-01` | success | pass | 1.209s | 0.242s | 3.000s | 3.000s | 6.242s | [28269487470](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269487470) |
| 02 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-02` | success | pass | 1.260s | 0.385s | 3.000s | 3.000s | 6.385s | [28269538334](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269538334) |

## Task Output Observed

### fxrun-drdave-TRX50-AI-TOP-flexnetos-01

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_f5ca91eb-99c6-490d-abbd-c918e45b3dfc","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:46:28Z"}
```

### fxrun-drdave-TRX50-AI-TOP-flexnetos-02

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_0c453d96-1ebc-48b7-adc6-b7c65ee77dcb","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:47:54Z"}
```

## Failures

No step failures recorded.

## Final GitHub Runner API Snapshot

```json
[{"busy":false,"id":4730,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","status":"online"},{"busy":false,"id":4731,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","status":"online"}]
```

## Metrics JSONL

```json
{"slot":"01","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-01.service","run_id":"28269487470","job_id":"83763710122","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269487470","dispatch_iso":"2026-06-26T22:46:23Z","created_at":"2026-06-26T22:46:24Z","updated_at":"2026-06-26T22:46:31Z","job_started":"2026-06-26T22:46:27Z","job_completed":"2026-06-26T22:46:30Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-01","timings_ms":{"dispatch_to_visible_ms":1209,"dispatch_to_created_ms":242,"pickup_latency_ms":3000,"exec_ms":3000,"total_ms":6242},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_f5ca91eb-99c6-490d-abbd-c918e45b3dfc","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:46:28Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:46:28Z","completedAt":"2026-06-26T22:46:28Z","durationMs":0},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:46:28Z","completedAt":"2026-06-26T22:46:28Z","durationMs":0},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:46:28Z","completedAt":"2026-06-26T22:46:28Z","durationMs":0}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is below 10s","end-to-end turnaround is below 60s"]}
{"slot":"02","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service","run_id":"28269538334","job_id":"83763864990","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269538334","dispatch_iso":"2026-06-26T22:47:49Z","created_at":"2026-06-26T22:47:50Z","updated_at":"2026-06-26T22:47:57Z","job_started":"2026-06-26T22:47:53Z","job_completed":"2026-06-26T22:47:56Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-02","timings_ms":{"dispatch_to_visible_ms":1260,"dispatch_to_created_ms":385,"pickup_latency_ms":3000,"exec_ms":3000,"total_ms":6385},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_0c453d96-1ebc-48b7-adc6-b7c65ee77dcb","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:47:54Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:47:54Z","completedAt":"2026-06-26T22:47:54Z","durationMs":0},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:47:54Z","completedAt":"2026-06-26T22:47:54Z","durationMs":0},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:47:54Z","completedAt":"2026-06-26T22:47:54Z","durationMs":0}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is below 10s","end-to-end turnaround is below 60s"]}
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

`/home/drdave/Desktop/meta/flexnetos_runner/_work/evals/20260626T224513Z`
