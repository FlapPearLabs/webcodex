# P1C acceptance — C2 scoped milestone, P1C incomplete

`P1C_COMPLETE=NO`。本里程碑关闭缺少远端 durable authority backend 的 Session shell open/exec 两项入口；其余七项仍为现有未迁移执行路径，等待长期监督/控制面架构裁定。accepted P1B 起点为 `f58e65c6d95bbd91165e97b4a98de694f97ae872`；其原 inventory、普通 ExecutionBroker、Git A/C 与 B22 生产契约不改。本记录不授权完整 P1C、Class B capabilities、P2 或合并。

## 实现与权限边界

named remote open/exec 在 prepare/connect/spawn 前固定拒绝：`remote_durable_authority_unavailable`、`command_started=false`。省略 resource 的 exec 从可信 stored executor 判断旧 SSH transport，在 output-limit 修改与 command write 前拒绝。只按 shell/session/project 精确身份清理 owned transport；status/close 保留。Runner withdraw SshPersistentShell capability，local PersistentShell 和 B22 SshShell 保留。

C2 专用 remote transport/preparer/availability 与孤儿 helper 一起退休，共享 SSH/B22 函数体不改。11 个完整 dedicated tests 退休；另一个 Windows 混合测试只退 remote half，保留 `windows_local_bash_override_still_refused` 的真实 handler 拒绝断言及 active_count=0。Windows native 为 NOT_RUN，退休测试不计新契约 PASS。

immutable P1B 的全部九个 C 逻辑 ID 保留。guard sparse overlay 仅为五个固定 source replacements、一个 retired origin、十项 exact raw removals、零 reference removals；未知/重复/缺项/错误 base/hash/targets 都拒绝。两轴 source 审查后主责仅转换 status，执行者没有 self-approve。完整门禁另找到过时 exec_ssh handoff tuple，只删除该五行 literal expected，扫描器和其余三项精确断言不改。

## 实际证据

| 检查 | 结果与范围 |
|---|---|
| 普通 production Runner build | PASS，Homebrew Rust 1.94.0、offline/locked、无 bootstrap，binary SHA `40b7c376e9fe40c83e4e42b750eabc213eaad183536b6a0315e53145ebb6118e` |
| 原生 macOS Runner register/poll/results | PASS，两个合法 open/exec 均固定拒绝、model SSH marker=0、capability=false、Runner/harness=0/0 |
| exact accepted-base old route，同一 no-effect assertion | EXPECTED RED，两条结果与 connect marker=1 收集后 Runner/harness=0/1；不是 compile/timeout/capability mismatch 先失败 |
| 完整默认 P1B guard + C2 overlay | PASS，18 passed/0 failed/2 ignored；source 前后相同，raw 260→250、reference 30→30、origin 626→625 |
| 原有 held-pipe 与 npm-pack 两项 ignored lanes | 分别显式执行，各 PASS，1 passed/0 failed/0 ignored，未把默认忽略当通过 |
| 实际 production Command::new+spawn 变异 | EXPECTED RED，普通 mutant check=0、同 production gate=101、新增两条 raw；独立 scratch target 避免缓存错扫 |
| 省略 resource 的 stored SSH fence 变异 | DIAGNOSTIC EXPECTED RED，Cargo=101、write count=2 对 expected=1；恢复精确 a501 源后同测试=0 |
| 新 remote authority fixture | DIAGNOSTIC，5/5=0；本地 persistent policy 与两项 capability tests 分别1/1=0 |
| 普通 overlay parser / 独立 parser audit | 普通3/3=0；历史独立4 tests含18额外拒绝变异 raw保留，范围仅 parser API |
| 普通 process library 回归 | PASS，38 tests；生产 process 字节未变，不能提升为新的 P1C lifecycle/security 证明 |
| 全库格式 / whitespace | PASS，真实 cargo fmt --all --check=0、git diff --check=0；保留此前 formatter 残留失败及精确撤回记录 |

Runner test target 的既有 `coding_agent.rs` `atomic_try_update` E0658 未修改。上述 Runner unit/spy tests 使用 child-local `RUSTC_BOOTSTRAP=1` 和 `-Zcrate-attr=feature(atomic_try_update)`，只计 DIAGNOSTIC；普通 production binary/native 证据没有 feature 注入。local fallback 没有 payload/profile-init marker positive control，仅 SOURCE_DERIVED_ONLY。Linux/Windows/real remote host runtime、八项 restart、PID reuse、whole-family 终态与 output continuity 都 NOT_RUN。

## 不完整条件与真正架构分歧

native macOS 固定 probes 反驳两项候选设计：payload setsid 成功离开 watchdog 原进程组；同解释器关闭指定 FD 后，备份 control writer 仍可交给其他 child。probe exit0 表示探测/清理完成，设计不变量结果是 FAIL；group escape 没有解除 Seatbelt 文件/网络限制。S1 无 production consumer 的原语已撤回并封存，当前普通 broker 是 accepted P1B 原字节。

新的 durable authority/restart grant validation、host-enforced whole-family supervision、可信 stateful shell control boundary、OutcomeUnknown/Orphaned、capture completeness 与八项 restart scenarios 尚未实现。C9=total9/migrated0/deferred fail-closed2/pending7/unclassified0；七项不能称关闭或安全通过。用户尚未选择精确关闭这些本地入口、保留 detached 的 host-enforced backend、或同时重新设计两类功能的控制面；不同路线改变长期信任边界，不能静默选择。

## 审查、存储与复现

当前源码绑定为 `evidence/recovery-current-source.json`，SHA `713cdfa7421f04dcba2619d1bcaa2b09f697f8388d8dae99241934f58a25de29`。它固定14个 changed source、11个关键 unchanged exact-base files 和 reviewed overlay；双轴 fresh review、完整 guard/raw 与新 diagnostic mutation 位于 reviews/evidence。预提交源码意见不等于未知 commit SHA 已被审查，implementation commit 的 exact-SHA 确认另行追加。

用户通过 antigravity 清理外部 work/ 与 target/ 时也删除了尚未入仓库的 P1C 报告/日志和 binaries；保留 outputs zip 只有 Slice 2A，不含 P1C。83份已复制证据逐哈希完好。旧5983 source-freeze和未复制签署文件不可用，没有伪造恢复；新意见从当前源重新核验，旧失败 stdout/stderr 的救出版本明确标为 tool-output recovered。普通 native 源/命令/退出 raw 已保留，binary 与 accepted-base scratch 原文件已 NOT_AVAILABLE，复跑需重新构建。

原生执行后仅有 cfg(test) macOS formatter 残留撤回与 p1b_guard 的五行预期修正，五个 production origin/fixture/parser 字节相同，不将旧执行改名为新时点。历史 native contract binding mismatch、早期缓存无效 NC、summary/raw 格式矛盾和原目录清理均记录于 history 文件，不能重写为干净成功故事。

主责承担最终 diff、权限判断、质量门禁、两轴审查与 Git 操作；Luna 只完成有界实现/机械验证。只有 C2 scoped milestone 的候选交付可提交，完整 P1C 保持未完成。未部署、未合并、未重写历史，不启动下一阶段。
