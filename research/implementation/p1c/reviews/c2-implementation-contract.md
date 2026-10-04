# Remote C2：最小实施形态冻结

签署：独立 Sol Spec / 安全架构审查上下文。起点 `f58e65c6d95bbd91165e97b4a98de694f97ae872`；当前 branch `impl/webcodex-p1c-durable-execution`。这是实施契约，不是未运行代码的 PASS。沿用 REMOTE-C2-DISPOSITION；不依赖 S1 是否提交。

## 已验证调用与影响

实际 CodeGraph 的 callers(open_ssh) 与 callers(exec_ssh) 均只有 `PersistentShellManager::handle_operation`（persistent_shell.rs:52）。impact 只进入该 facade 的测试 adapter 和 `dispatch_request_with_outcome`（dispatch.rs:350）。callees 证明 open 的 remote prepare / spawn 入口是 `RemoteShellTransport::spawn`，exec 的写入口是 `ProcessManager::exec`。原始 toolresult 保存于 REMOTE-C2-CALLER-IMPACT-RAW.json。restricted 读数据库失败后，以授权提升进行只读索引查询成功；未运行 init / 更新索引。

额外边界：local `exec`（persistent_shell.rs:437）先取 ShellSummary，再调用 `validate_open_shell_boundary`（:1005），后者只检查 cwd/profile。若已有 remote transport 的 cwd/profile 恰好合法，省略 ssh_resource 可进入本地分支写入这个 transport；请求标记不是可信 transport 身份。

## 固定关闭形态

1. 在 `handle_operation` 得到 operation 与 ssh_resource 后、`validate_boundary` 或任何 SSH prepare 之前，对 `ssh_resource.is_some() && action in {open, exec}` 返回固定错误 code=`remote_durable_authority_unavailable`，message 说明 remote durable authority backend unavailable。close 仍走既有 exact identity close；status 只观察或现有安全 cleanup。
2. 拒绝 exec 时允许调用 `processes.close(shell_id, workflow_session_id, runtime_project_id, reason)` 释放恰为该 identity 的旧 owned transport。失败也继续稳定拒绝，不恢复 exec、不新连接。不能把 local transport close 等同远端进程全族终止。
3. 对没有 ssh_resource 的 exec，在已有 `exec` 中读出 summary 后、任何 set_output_limit / `processes.exec` 写入前，若 summary.executor == `"ssh"`，同样固定拒绝并 exact owned cleanup。或把这一可信已有 transport 身份检查集中到 handler 的 exec 路径；不得只检查 request 字段。
4. 两个 cfg 分支 `open_ssh` 及 `exec_ssh` 的旧执行体替换为上述固定拒绝结果。保留现有 private helper signature/caller 即可，不新增泛化 launcher；不得残留 `RemoteShellTransport::spawn`、bootstrap、`processes.exec` 的不可达旧执行体。直接 private 调用也必须 fail closed。
5. 删除 main.rs 对 `RunnerCapabilityId::SshPersistentShell` 的可执行广告，保留 SshShell、local PersistentShell 及 B22 one-shot/background SSH 行为。
6. 删除此变更产生的未使用 import、EXECUTOR_SSH 常量与 PersistentShellManager.ssh_pool 字段。constructor 的 pool 参数也机械删除，并更新唯一生产调用点 transport.rs:221 与该专用测试内的构造；不改 JobManager 的 SSH pool、ssh.rs 或 remote_shell.rs 的 B22 / transport 实现。
7. 旧 remote launch 站点及 accepted logical ID 仍以 C2 / DEFERRED_FAIL_CLOSED 记录，next contract 沿用 REMOTE-C2-DISPOSITION。不能从 P1B accepted inventory 删除或重分类到 B。

## 允许文件

- `crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs`：三个拒绝边界、移除 private helper 旧执行体、产生的 orphan state/import 清理及 dedicated tests。
- `crates/webcodex-runner/src/main.rs`：只删除该 capability 广告与直接对应说明。
- `crates/webcodex-runner/src/webcodex_runner/transport.rs`：只机械更新 facade constructor 的 SSH pool 参数。
- 外部 review/evidence；最终 P1C inventory/anti-bypass evidence 留后续固定验收，不改 accepted P1B JSON。

## 必须正负向与真实 RED

- named remote open：合法 project/resource、spy executable / prepare marker fixture；稳定拒绝，spawn/connect/prepare marker 均不存在，active_count 不增加。非法或缺配置也不得先 materialize config/credentials。
- named remote exec：现有 owned spy remote transport，write_count 保持 0；恰当 identity cleanup 幂等，不能关闭其他 entry。
- omitted-marker exec：已拥有 summary.executor="ssh" 的 spy transport + ssh_resource=None + 恰好合法 cwd/profile；稳定拒绝、write_count=0，专门覆盖旁路。
- direct private open_ssh / exec_ssh：直接调用只返回固定拒绝，无 create/connect/write。
- close/status：只读或 exact owned local cleanup；保留历史 terminal result，不能产生执行。fixture-owned unrelated entry 仍存活。
- capability：实际 supported_capabilities 构造不广告 SshPersistentShell；仍广告原 SshShell/local PersistentShell。
- local smoke：现有合法 local open/exec/close 行为保持；B22 只取必要受影响 regression，不修改其 authority。

负控要在外部同源 scratch 恢复真正旧路由与旧 helper 执行体；只移除 handler guard 而 private helper 仍拒绝不是有区分力的负控。要求同一 marker/write 行为断言 RED 且 exit 非零；记录 mutation diff/hash、命令、退出与 source restore。omitted-marker 的 fence 也独立移除并使 spy write 断言 RED。ENV_BLOCKED / NOT_RUN / HOST_UNAVAILABLE 不计 PASS。

实施后由独立审查按 exact final source/hash 检查。新 backend 未实现、未运行 remote host 的状态不能提升为 MIGRATED 或 native runtime PASS。
