# P1C durable surface inventory — C2 milestone

基线为 `f58e65c6d95bbd91165e97b4a98de694f97ae872`。本表保留 accepted P1B 的九个逻辑 ID；`evidence/baseline-surface-matrix.md` 是该 SHA 的完整调用链、FD、所有权与 authority 基线，行号不是退休后代码的当前位置。

当前 P1C 未完成：migrated=0，deferred fail-closed=2，architecture decision pending=7，unclassified=0。Pending 七项仍保留现有执行路径，不能计为关闭或安全通过。

| ID | 本里程碑处置 | 下一契约 |
|---|---|---|
| `shell.persistent-local.unix` | `ARCHITECTURE_DECISION_PENDING` | Trusted control plane must be separated from arbitrary stateful interpreter capability. |
| `job.detached-payload.unix` | `ARCHITECTURE_DECISION_PENDING` | Host-enforced job-family ownership, current-authority revalidation, exact process incarnation and honest restart/output outcomes. |
| `job.detached-supervisor.unix` | `ARCHITECTURE_DECISION_PENDING` | Host-enforced job-family ownership, current-authority revalidation, exact process incarnation and honest restart/output outcomes. |
| `shell.persistent-remote.unix` | `DEFERRED_FAIL_CLOSED` | Remote host/path, credential audience, network authority, per-job confinement and uncertain channel outcome must be independently established before reopening. |
| `shell.persistent-local.windows` | `ARCHITECTURE_DECISION_PENDING` | Trusted control plane must be separated from arbitrary stateful interpreter capability. |
| `job.detached-payload.windows` | `ARCHITECTURE_DECISION_PENDING` | Host-enforced job-family ownership, current-authority revalidation, exact process incarnation and honest restart/output outcomes. |
| `job.detached-supervisor.windows` | `ARCHITECTURE_DECISION_PENDING` | Host-enforced job-family ownership, current-authority revalidation, exact process incarnation and honest restart/output outcomes. |
| `shell.persistent-remote.windows` | `DEFERRED_FAIL_CLOSED` | Remote host/path, credential audience, network authority, per-job confinement and uncertain channel outcome must be independently established before reopening. |
| `job.detached-watchdog.unix` | `ARCHITECTURE_DECISION_PENDING` | Host-enforced job-family ownership, current-authority revalidation, exact process incarnation and honest restart/output outcomes. |

远端两个 ID 的当前路径是 typed PersistentShell operation → Runner handler → 固定 `remote_durable_authority_unavailable`；私有 remote helpers 同样拒绝。原专用 `remote_shell.rs` transport 与 preparer 已退休，绝不将其文件消失解释为历史 ID 消失。

accepted P1B 的 123 logical production surfaces、35 model execution authority、A4/B22/C9、fixed D4 原账本与 JSON 原字节不变。当前 production AST 的变化必须且仅为 raw 260→250、references 30→30、origins 626→625。它们与 logical surface counts 属于不同计量单位。

B22 保持原分类和 NOT_PROVEN，未归入 P1C，也未因本次远端持久入口拒绝而宣称 one-shot/background SSH 已受 P1C confinement。
