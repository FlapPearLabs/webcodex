# 当前 macOS 监督边界与实施选择

审查角色：独立 Sol Spec / 安全架构上下文。原始 P1C 用户请求全文已读取；accepted base 为 `f58e65c6d95bbd91165e97b4a98de694f97ae872`。仓库只读；本次只写外部 evidence/review，并实际复跑两个既有合成 fixture。

## 判断

当前 Seatbelt + POSIX process group/session + watchdog + direct Child/native birth fences，不能在允许任意 payload/descendant 的条件下证明取消、TTL 或授权撤销使原作业全族终止。不引入更强的宿主执行族边界时，不能按 S2/S4 的原全族终态条件签 PASS。

这不是“macOS 上任何可能架构都做不到”的断言；结论针对现在实际可用、已选的机制。per-job filesystem/network sandbox 与 lifetime supervision 是不同证明：离组没有解除 Seatbelt，但已足够破坏 group-based 全族终止与授权撤销后的停止保证。

## 独立原生证据

固定 fixture `architecture/lifecycle-probe/src/main.rs` 实際调用当前 S1 原语，将复制到授权临时根的自身 payload 加入 owned leader 的组，再由 payload 调用 libc::setsid。以 `rustup run 1.98.1 cargo run --offline --locked --manifest-path .../Cargo.toml` 在提升后的 native macOS 上运行：

```text
owner_group=39981
payload_pid=39982
setsid_result=39982
payload_group_after=39982
payload_session_after=39982
```

Cargo exit=0，fixture cleanup 完成；涉及 source before==after。记录、stdout/stderr 与原始 toolresult 分别为 `lifecycle-probe/independent-native-setsid.json`、`*.stdout.log`、`*.stderr.log`、`independent-native-setsid-toolresult.json`。exit 0 只表示复现/cleanup 成功，不能解释成 lifetime invariant PASS。本次并未把“原组消失而 escaped payload 仍活”的完整取消探针伪装成已跑；已跑事实是 actual sandbox payload 的 setsid 成功并离组，取消仅杀原组不能覆盖它的结论来自实际 OS API语义和源代码。

Apple 文档说明 setsid 建立以调用者 PID 为 ID 的新会话和组；kill(0) 只向发送者当前组发信号，负 PID 也只向匹配组发信号：[setsid(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setsid.2.html)、[kill(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kill.2.html)。当前源 `crates/webcodex-process/src/unix.rs` 的 signal_group / group_has_live_members 只追踪固定 pgid。这解释了反证，没有要求更改 accepted P1B。

第二个固定 fixture `architecture/fd-closure-probe.sh` 只创建临时空 control 文件、使用 bash EXIT trap 删除，不读取真实环境秘密或项目资料。native macOS 独立复跑 exit=0，stdout 为：

```text
probe_descriptor_12probe_descriptor_10unrelated_descendant_reached_control
```

原脚本先对 current-shell eval 关闭 7/8/9；bash 在可观察的备份 FD 上仍保存 control writer，payload 可将 FD 10 复制到 99 后交给无关 /bin/sh。证据为 `independent-native-fd-closure.json` 与 stdout/stderr logs；source SHA-256 为 `735ab7cdb1db78de3da47e447d9f2cc916ddc18397fcb41f7fecd4f7f7d559cb`，before==after。此探针反驳“仅在 current interpreter 的 eval 上关闭数字 FD 就能隔离 writer”的修补路线，不声称已实现或验收新 SessionShell 控制架构。

## 合同受影响处

| 判据 | 当前证据/机制 | 结论 |
|---|---|---|
| payload 初始加入 owned private group | S1 source；既有 narrow native；actual 同源 fixture | 窄原语有用途，但不证明长期成员不可离组 |
| filesystem/network 权限不被后代扩大 | per-job Seatbelt；既有窄 kernel tests | 离组本身不解除 sandbox；此次未宣称所有 escaped descendant 限权场景均测完 |
| cancel/TTL/revocation 全族结束 | kill(-pgid)/kill(0)；actual payload setsid 成功 | 当前设计无法作全族 PASS |
| group gone/direct exit/pipe EOF 推导全族结束 | pipe 可关闭，组可改变；S4 要求族 quiescent | 不足以生成已证明的 Cancelled/Succeeded/Failed |
| protected writer 不泄漏 | 同解释器 eval 关闭 FD；原生合成 writer 泄漏 | 该路线 FAIL；需要控制面与任意 payload 不共享 interpreter/control capability |
| OutcomeUnknown/Orphaned 诚实投影 | S4 contract | 可防伪造结果，不能替代取消全族保证 |
| C2 remote 执行关闭 | 独立 signed defer 与 concrete freeze | 可继续，无须等本地架构选择 |

## 最小可选路线

1. 精准 fail-closed defer：在真正 launch/exec 前关闭不能满足合同的 local C surfaces，保留必要历史观察与仅强身份绑定的 owned cleanup；逐条明确 DEFERRED_FAIL_CLOSED 与 next contract，不重分类 B22，不假称远程或全族已停止。可不引入新长期宿主边界；原 P1C 的 restart/full-family 实施条件仍不能从“全都 deferred”自动变成完成。
2. 要保留 detached 任意 payload 执行：单独设计并授权一个由宿主强制维持 job family membership、跨 Runner 生存且能在崩溃后证明 whole-family termination 的 backend。仅新增一个普通 supervisor、改变 setsid/setpgid 顺序、轮询父 PID、扫描当前组、或搜索/禁止某个 setsid executable 都不具备此性质。该 backend 是新的长期可信边界，不能从现 P1C 的窄 group 原语静默扩张实施。
3. local SessionShell 若保留跨命令 cwd/env/function 等 current interpreter 状态，还需新的可信控制协议/解释器边界，使任意模型代码无法拥有或复制 protected writer。将 command 放进隔离 child 可以隔离 FD，但会改变这些 stateful semantics，不能宣称语义原样保留。

用户的两项异步架构选择未回复；S2/S3/S4 依赖实现应等待。远程 C2 已有独立授权，可以推进。

## S1 提交处置

建议暂不提交当前没有 production consumer 的 public S1 API。当前真实生产引用扫描只找到定义、说明和 dedicated tests；C9 尚无迁移。根据 AGENTS.md 的“具体当前需求、最小解、无 speculative abstraction”原则，在宿主 backend / defer 路线未选前，单独提交会把可能不再需要的 API 固化，并不能交付用户请求的监督语义。

保留 hash-pinned reviewed patch 和外部证据即可；不是删除或重置用户源码。若后续选择实际复用该窄原语，再把它与已签 consumer 放入 bounded milestone、补精确新源码下必要 runtime/NC/source evidence，并做 exact final SHA 的 fresh review。独立 remote defer 可自行成为 milestone。

## 完成诚实边界

封存后的时点补充：上述 setsid native record 发生于主树仍含 mod974 prototype 时。主负责人随后只撤回两个 hash 匹配的自有 S1 实验文件，完整974/230源与patch已封存于 `s1/sealed-prototype/`；独立再次确认主树mod与accepted base相同，prototype test不在主树。固定 lifecycle-probe 当前仍引用封存 API，不能直接对已恢复的主树重跑；后续复跑必须使用外部同源scratch应用sealed patch，再固定source/runtime证据。窄S1结论和historical06/current974证据分界完整记于 `s1/INDEPENDENT-SCOPE-REVIEW.md`，没有把旧原生运行重命名为sealed后新运行。

本次完整读取：原 P1C 请求、repo AGENTS/CONTRIBUTING/TESTING、clean-room/S1/S2/S4/remote signed contracts、S1 生产 diff、整份 dedicated tests、S1 relevant raw/records与两个 fixed fixture。原 accepted implementation 的 large modules 只读相关函数/调用图，未声称全仓审计。独立实际运行：同源 setsid fixture、原 synthetic FD script；既有 S1 native/NC 未被本次重新全跑。未运行 Linux/Windows 或 real remote host，均不能标 runtime PASS。未改仓库代码、P1B、coding_agent、B22；未提交/推送；只新增外部 review/evidence。Skills 实际使用：code-review 的 clean-room/Spec 对照与 specialized-domain-audit 的不变量/原始证据方法；本上下文承担 Spec/security 轴，未伪称独立 Standards 聚合已完成。
