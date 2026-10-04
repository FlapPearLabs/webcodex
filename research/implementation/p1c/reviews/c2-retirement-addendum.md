# C2 精确补充签署：第十一项测试与第十项 raw

独立 Sol Spec/security 签署，2026-10-04，base `f58e65c6d95bbd91165e97b4a98de694f97ae872`。本追加不覆盖先前签署及候选记录；修正其最终候选全集为十项 raw / 零项 reference，以及十一项 dedicated 测试退休。final source/scanner/runtime 仍待冻结验收。

1. 接受退休 `crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs::windows_named_ssh_persistent_shell_real_transport_opt_in`。已完整核读 exact baseline 第 1934–2187 行：remote open 成功后观察 state/cwd/env/functions/output、generation/resource invalidation、timeout/exit/close；缺 `WEBCODEX_TEST_WINDOWS_SSH_HOST` 时 print-and-return。它依赖当前明确关闭的 remote backend，属于 C2 专用 historical fidelity，与 B22 one-shot/background 无关。完整 body 已保存在 `c2-baseline-archive/crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs`，hash 见 archive manifest。连同先前九个 Unix fidelity 与 Windows preparer，退休总数为十一项；处置为 `RETIRED_UNAVAILABLE_CONTRACT / HISTORICAL_NOT_RUN`，不得称 Windows runtime PASS。
2. 接受退休同文件 `summary_error_result_from_status` 与紧贴其定义的专用说明。indexed-base CodeGraph 唯一 caller 是 `PersistentShellManager::exec_ssh`；exact base 第 296/305/314 行均在该方法中，current source 已无定义或引用。helper 内唯一 `ProcessManager::status` 观察行是 immutable inventory 中另一个 NON_PROCESS row。它是本次关闭 exec_ssh 直接产生的孤儿，无当前 local/B22 caller。
3. 增加且只增加这一精确 raw removal 键：file=`crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs`；symbol=`crate::webcodex_runner::persistent_shell::summary_error_result_from_status`；primitive=`method::status`；count=`1`；targets=`["webcodex-runner:webcodex-runner"]`。旧九项保留，因此最终候选十项。不存在新的 reference deletion。

完整十项候选保存为 `c2-baseline-archive/signed-retirement-row-candidates-v2.json`；此前 nine-row 文件保留作为审查历史，不得伪称它已经完整描述最终 scanner delta。新增独立源/CodeGraph toolresult 保存于 `C2-RETIREMENT-ADDENDUM-RAW.json`。主负责人已明确批准这两项窄补充。

本签署不批准任意 orphan prune，不放宽五项 replacement/唯一 origin removal，不改变 immutable P1B 或 C9/B22 账本。若最终 scanner 出现十项/零项之外的差异，依然应停止自动接纳、逐项独立复核。C2 两项 DEFERRED_FAIL_CLOSED、C7 PENDING 与 P1C_COMPLETE=NO 的完成边界保持。
