# P1C acceptance — incomplete C2 milestone candidate

`P1C_COMPLETE=NO`。本记录不是完整 P1C 验收，不允许启动 Class B capabilities 或 P2。accepted P1B 起点为 `f58e65c6d95bbd91165e97b4a98de694f97ae872`；该 JSON 与普通 ExecutionBroker 源保持原字节。

本候选只有远端持久 Session shell 两项 `DEFERRED_FAIL_CLOSED`；其余七项仍 `ARCHITECTURE_DECISION_PENDING`，未迁移、未关闭。新长期 backend / 可信 stateful shell 控制面的选择没有得到用户裁定。

另一个 Windows 混合测试保留仍有效的 local Bash 拒绝断言，改名 `windows_local_bash_override_still_refused`；只退休其旧 remote missing-resource 半段。因此是 11 个完整 dedicated tests 退休，加 1 个混合测试的 remote half 退休 / local half 保留，不能记录为第十二个完整测试获准删除。此处为 source/fixture 修正，Windows native runtime 仍 NOT_RUN。

## 候选交付

- named remote `open`/`exec` 固定拒绝，私有 helpers 固定拒绝，withdraw `SshPersistentShell` capability。
- 无 resource 的 exec 仍检查可信 stored executor，旧 SSH transport 不接收 command write；只按 exact owned identity 清理，status/close 保留。
- C2 专用 transport/preparer/availability、其孤儿 helper 与 11 个依赖成功执行的 dedicated 测试一起退休；共享 B22 SSH 路径保留。
- immutable P1B 的九项 Class C 历史 ledger 保留；guard 仅应用五项 replacement、一项 origin removal、十项 raw removal、零 reference removal 的固定 overlay。

## 验收状态

正式候选 source manifest、原生 requests、真实非零负对照、完整 guard、两轴独立 review 尚待最终冻结。本文件先明确候选范围，不把待执行项目标为 PASS。

正常 Runner 生产编译可使用 Homebrew Rust 1.94.0。Runner test target 的既有 `coding_agent.rs` `atomic_try_update` E0658 未改；若显式 bootstrap 注入 feature 执行测试，结果只计 DIAGNOSTIC，不作为无修改原生生产二进制证据。

## 完整 P1C 尚未满足

native macOS `setsid` 固定探针反驳以旧 group 证明整族监督的路线；control writer fixture 反驳在 current interpreter 上关闭指定 FD 即隔离控制能力的路线。这两项是设计反证，不是 ENV_BLOCKED。

当前未实现新的 durable authority/restart grant validation、host-enforced whole-family supervision、可信 stateful shell control boundary、required eight restart scenarios、PID reuse refusal、OutcomeUnknown/Orphaned 与 capture completeness 投影、跨 Runner output continuity。本候选不能填补这些条件。Linux/Windows/real remote host runtime 全部 NOT_RUN。

不合并、不重写 accepted P1B、不开始下一阶段。架构裁定后应复核后续契约是否仍可使用此前封存的窄 primitive，再按消费者、正负证据和 exact SHA 两轴审查逐片推进。
