# FlexNetOS Runner Evaluation — 20260626T224057Z

- repo: `FlexNetOS/flexnetos_runner`
- workflow: `runner-smoke.yml`
- ref: `main`
- isolate peers: `1`
- started: `2026-06-26T22:40:57Z`

## Live Results

| Slot | Runner | Conclusion | Accuracy | Dispatch→visible | Dispatch→created | Pickup latency | Exec time | Total | Run |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| 01 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-01` | success | pass | 1.163s | 0.889s | 52.000s | 3.000s | 55.889s | [28269327163](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269327163) |
| 02 | `fxrun-drdave-TRX50-AI-TOP-flexnetos-02` | success | pass | 1.236s | 0.678s | 3.000s | 3.000s | 6.678s | [28269411223](https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269411223) |

## Task Output Observed

### fxrun-drdave-TRX50-AI-TOP-flexnetos-01

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_d2e41ac9-021c-49f8-a330-86f7172c05d7","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:43:02Z"}
```

### fxrun-drdave-TRX50-AI-TOP-flexnetos-02

```json
{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_27536467-d861-4e4b-9c18-2f47bd96cc1b","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:44:27Z"}
```

## Failures

No step failures recorded.

## Final GitHub Runner API Snapshot

```json
[{"busy":false,"id":4730,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","status":"online"},{"busy":false,"id":4731,"labels":["self-hosted","Linux","X64","local","flexnetos"],"name":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","status":"online"}]
```

## Metrics JSONL

```json
{"slot":"01","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service","run_id":"28269327163","job_id":"83763238085","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269327163","dispatch_iso":"2026-06-26T22:42:08Z","created_at":"2026-06-26T22:42:09Z","updated_at":"2026-06-26T22:43:05Z","job_started":"2026-06-26T22:43:01Z","job_completed":"2026-06-26T22:43:04Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-01","timings_ms":{"dispatch_to_visible_ms":1163,"dispatch_to_created_ms":889,"pickup_latency_ms":52000,"exec_ms":3000,"total_ms":55889},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-01","expected_slot":"01","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_d2e41ac9-021c-49f8-a330-86f7172c05d7","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-01-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:43:02Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:43:02Z","completedAt":"2026-06-26T22:43:02Z","durationMs":0},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:43:02Z","completedAt":"2026-06-26T22:43:02Z","durationMs":0},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:43:02Z","completedAt":"2026-06-26T22:43:02Z","durationMs":0}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is elevated; inspect capacity, queued work, or GitHub queueing","end-to-end turnaround is below 60s"]}
{"slot":"02","runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","unit":"actions.runner.FlexNetOS.fxrun-drdave-TRX50-AI-TOP-flexnetos-02.service","run_id":"28269411223","job_id":"83763486112","run_url":"https://github.com/FlexNetOS/flexnetos_runner/actions/runs/28269411223","dispatch_iso":"2026-06-26T22:44:22Z","created_at":"2026-06-26T22:44:23Z","updated_at":"2026-06-26T22:44:30Z","job_started":"2026-06-26T22:44:26Z","job_completed":"2026-06-26T22:44:29Z","conclusion":"success","accuracy":"pass","work_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work","install_dir":"/home/drdave/Desktop/meta/flexnetos_runner/_work/repos/actions-runner-02","timings_ms":{"dispatch_to_visible_ms":1236,"dispatch_to_created_ms":678,"pickup_latency_ms":3000,"exec_ms":3000,"total_ms":6678},"assertions":{"expected_runner_seen":1,"actual_runner_seen":1,"workspace_seen":1},"task_output":{"expected_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","actual_runner":"fxrun-drdave-TRX50-AI-TOP-flexnetos-02","expected_slot":"02","runner_os":"Linux","runner_arch":"X64","runner_tracking_id":"github_27536467-d861-4e4b-9c18-2f47bd96cc1b","runner_workspace":"/home/drdave/Desktop/meta/flexnetos_runner/_work/actions-runner-02-work/flexnetos_runner","hostname":"drdave-TRX50-AI-TOP","whoami":"drdave","date_utc":"2026-06-26T22:44:27Z"},"steps":[{"name":"Set up job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:44:26Z","completedAt":"2026-06-26T22:44:27Z","durationMs":1000},{"name":"Verify runner identity and repo-local paths","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:44:27Z","completedAt":"2026-06-26T22:44:27Z","durationMs":0},{"name":"Complete job","status":"completed","conclusion":"success","startedAt":"2026-06-26T22:44:27Z","completedAt":"2026-06-26T22:44:27Z","durationMs":0}],"failures":[],"lessons":["identity and repo-local workspace assertions passed","workflow completed successfully","runner pickup latency is below 10s","end-to-end turnaround is below 60s"]}
```

## Lessons Learned

- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] identity and repo-local workspace assertions passed
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] workflow completed successfully
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] runner pickup latency is elevated; inspect capacity, queued work, or GitHub queueing
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-01] end-to-end turnaround is below 60s
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] identity and repo-local workspace assertions passed
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] workflow completed successfully
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] runner pickup latency is below 10s
- [fxrun-drdave-TRX50-AI-TOP-flexnetos-02] end-to-end turnaround is below 60s

## Artifact Directory

`/home/drdave/Desktop/meta/flexnetos_runner/_work/evals/20260626T224057Z`
