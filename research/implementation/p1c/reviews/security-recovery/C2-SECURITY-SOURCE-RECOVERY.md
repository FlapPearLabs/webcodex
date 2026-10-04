# C2 当前字节独立源码接受：外部证据清理后的新绑定

**源码接受：ACCEPTED_FOR_EXACT_FROZEN_C2_SOURCE，blocking_findings_remaining=[]，full_p1c_decision=NO。** 这是基于当前实际源码与仓库保存的证据副本作出的新意见，不是恢复或重建已丢失的旧签署。正式 C2 milestone 仍等新的完整 guard、最终交付清单与 exact commit SHA。

base/HEAD `f58e65c6d95bbd91165e97b4a98de694f97ae872`，branch `impl/webcodex-p1c-durable-execution`。新 current-source binding 的实际 SHA 为 `713cdfa7421f04dcba2619d1bcaa2b09f697f8388d8dae99241934f58a25de29`。我独立验证 14 项 changed/new/source 删除绑定、11 项关键 P1B/C7/lock/coding_agent/macOS child 与 exact base 的字节相等；五项生产源、fixture、parser pins 都与此前独立审查相同。

完整 guard 此前暴露过期期望：platform handoff 测试仍要求已退休 exec_ssh 的 method::exec tuple。当前唯一新代码差异是删去这个精确 tuple 的五行；将其放回可精确重现旧 guard whole-file hash `bcf67312...`，当前为 `ca7a5adb...`。其余测试/精确比较/生产 scanner 均保留，没有新增退休或放宽规则。本次实际全库 fmt 与 diff check 均退出0。

当前 overlay 为 `a12cecad...`；把唯一 status literal 反向换回 candidate 可重现此前 `1016b312...` 原字节 hash。固定五 origins/一个退休 origin/十 raw/零 ref、immutable P1B 字节、C9 原始全部 rows与C7活跃来源均已当前核验。Windows mixed test 的 local handler Bash 拒绝与 active_count0 保留，账本为11 whole + 1 partial，不能新增第十二个 whole retirement。

用户确认清理 external work/target 后，旧5983 manifest、两轴旧外部报告、native binaries 与 accepted-base scratch 不再可读取。没有据此捏造文件或恢复旧 hash 签署。仓库83项 curated artifacts 全部可读取，逐项 hash 与保存 manifest 匹配；native build/commands/results/exit/traceback/harness copies 完整核验。生产40b7记录 ordinary build、请求2/拒绝/marker0/Runner0/harness0；accepted-base旧 route记录请求2/connect marker1/Runner0，同一 no-effect 断言 actual harness1，满足 AND 负控。binary与626 origin原件曾在本轮独立验证，当前只引用已保存的来源与raw记录，并明确原件不可用。local fallback 仍 SOURCE_DERIVED_ONLY，Windows/Linux/real remote runtime 仍 NOT_RUN。

这份意见不把过期 metadata 视作新 NC 运行时点，不把旧格式失败或默认 ignored lanes 当作 PASS。默认 guard 共20测试，其中2项是 accepted P1B 原有显式 held-pipe与npm pack lanes；修复后默认结果应明确为18pass/0fail/2ignored，若显式补跑则逐项实际核验。新的完整结果尚未到达。

C2只是两条远端入口 DEFERRED_FAIL_CLOSED；其余C7仍 active ARCHITECTURE_DECISION_PENDING，migrated0、unclassified0。长期 authority fork 未由用户选择，完整P1C不通过，不开始下一阶段。repo保持只读，仅在新外部恢复目录写审查文件。
