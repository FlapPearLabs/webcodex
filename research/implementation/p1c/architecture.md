# P1C architecture — accepted C2 boundary and pending local design

本文件记录 C2 最小关闭里程碑及未完成边界，不宣布完整 P1C 验收。起点 `f58e65c6d95bbd91165e97b4a98de694f97ae872`。P1B 普通 ExecutionBroker、ManagedChild、Git A/C、Class B 权限契约保持原样。

另一个 Windows 混合测试保留仍有效的 local Bash 拒绝断言，改名 `windows_local_bash_override_still_refused`；只退休其旧 remote missing-resource 半段。因此是 11 个完整 dedicated tests 退休，加 1 个混合测试的 remote half 退休 / local half 保留，不能记录为第十二个完整测试获准删除。此处为 source/fixture 修正，Windows native runtime 仍 NOT_RUN。

## 已决定的远端持久边界

本地 sandbox 不能建立远端 host/path 的 confinement。SSH credential audience、网络与 remote authority backend 尚未定义，因此 remote persistent `open`/`exec` 在任何 prepare/connect/spawn 前拒绝，固定错误为 `remote_durable_authority_unavailable`，`command_started=false`。Runner 不再公布 `SshPersistentShell` capability；local PersistentShell 与 B22 one-shot/background SshShell 不改契约。

请求省略 resource 标记不能绕过关闭：exec 从 manager 的可信 summary 获取 executor，已有 `ssh` transport 在 output-limit 修改和 command write 前被拒绝。允许仅按 shell/session/project 三项精确身份清理本地 owned transport。status/close 保留历史观察和幂等 terminal receipt；本地 transport shutdown 不证明远端全部后代已结束。

退休的 remote transport 模块、专有 preparer/availability 与唯一 exec_ssh helper 无其他生产调用者。11 个成功/fidelity 测试随 unavailable contract 退休，其 exact-base 源仍可由 Git 起点读取，不改成空断言、不标 PASS。capability 的两项 existing 测试保留，实际 builder 必须 false。

## 必须裁定的本地长期边界

native macOS 固定探针实测 payload `setsid()` 成功并离开 watchdog 的原进程组。原组 kill 与 group-gone 不能证明 whole-family termination；这没有解除 Seatbelt 文件/网络限制，也不是已证明 sandbox escape。

另一个 synthetic control fixture 实测，在 current interpreter 的 eval 上关闭 FD7/8/9，bash 的备份 writer 仍可复制到 FD99 并交给其他 child。该证据反驳仅关闭数字 FD 的修补方案，不宣称已实现可信 Session shell 协议。

用户尚未选择：精准关闭这些本地执行入口；保留 detached 并授权 host-enforced family backend；或保留两类功能并同时重新设计控制面。Pending 七项仍未迁移。不得把所有入口 deferred 自动视为满足 restart、PID reuse、OutcomeUnknown 与全族监督条件。

## 后续实施契约，当前未启用

authority 必须由当前 registry 中 exact accepted project identity 建立 canonical roots、executable/toolchain、明确 env/network、credential audience 与 lifecycle incarnation；cwd、child 输出路径和 persisted path 均不能恢复 grant。恢复次序为 persisted claim → 当前 grant 重验 → exact birth/boot/lock identity → 观察同一 execution 或 quarantine/refuse；不以 PID alone attach 或 signal。

可信 supervisor 可保留在 payload sandbox 外以维护 durable control state，但只能执行固定单 payload、经重验和 per-job brokered launch 的契约，不能成为通用 unconfined execution oracle。不同 job 不共享 union roots。普通 ManagedChild Drop 行为不能代替 durable ownership。此前无 production consumer 的 S1 窄原语已封存为 patch，不进入本里程碑 API。

output 的 EOF、capture 完整性、tail truncation 与 payload outcome 必须分开。Unknown/Orphaned 不生成虚构 exit code、Succeeded 或 Failed；lost acknowledgement 不重放 payload；取消仅在正确 whole-family quiescence 证明后终态。八项 required restart scenarios、原生 Linux/Windows 生命周期及远端 confinement 当前均 NOT_RUN。

## 结构门禁

保留 immutable P1B inventory，增加精确 C2 overlay：五个固定 source replacements、一个固定 retired origin、十个精确 raw removals、零 reference removals。其余 classification、targets、origins、body、non-Rust 与 release obligations 不变。未知/重复/缺项/旧 hash 不符必须拒绝；不得 wildcard 或运行时自动采纳新 source。仅本里程碑 remote closure 得到审查后状态，门禁通过不等于完整 P1C 通过。
