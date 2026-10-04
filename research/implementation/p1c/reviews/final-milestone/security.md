# C2 限定里程碑独立安全审查

结论：`PASS_C2_SCOPED_MILESTONE`，当前剩余阻断项 0。仅接受缺少远端 durable authority backend 时关闭 persistent SSH open/exec 的 C2 交付。**完整 P1C = NO**，七项本地/ detached 路径仍未迁移，用户长期 trust boundary 选择未答，未授权下一阶段、合并或部署。

基线为 `f58e65c6d95bbd91165e97b4a98de694f97ae872`。最终冻结 `final-candidate-files-v2.json` SHA `37499fe48e2686303c2c62d402c91e359f9412b1050a6dd76eeb77b4d7b1e08e` 的 165 个路径逐字节匹配；源码绑定 `713cdf…` 的14项及11项关键 exact-base 文件都正确。五项 source replacements、一个 retired origin、十项 raw removals、零 reference removals 的 fixed overlay 接受状态仅由主责转换。C9原始完整行保留，migrated0、remote deferred2、active architecture pending7。

固定拒绝发生在 named remote prepare/connect/spawn 前；省略 resource 的请求检查可信 stored executor，在修改 output limit/写命令前拒绝。只按 shell/session/project 清理 owned transport，status/close 维持原范围。SshPersistentShell capability 撤回，本地 persistent 与 B22 SSH 契约保留。11个完整 dedicated fidelity tests 退休，Windows mixed test 仅退 remote half；存活 local handler 拒绝与 active_count=0 断言保留。最后 F-ST4 修正仅移除五行已签退休 exec_ssh tuple，扫描器及其余断言未放宽。

实际普通 macOS production binary `40b7…` 的两条合法请求均固定拒绝，model marker0、Runner/harness0。Exact accepted-base 同一 no-effect assertion 收到真实 connect marker1 后 harness1，Runner0，满足“marker正向 AND 同一断言实际非零”，没有把编译/超时或 capability 差异单独当负对照。完整普通 guard 默认18/0/2 ignored；原两项 ignored 分别显式执行各1/0/0、exit0，实际20项跨三轮执行。production launcher mutant普通 check0、同 gate101、新增2 raw，恢复精确源码。

恢复后的 stored SSH fence 负控只删除九行 executor guard。独立内存重建得到实际 mutant hash `d8fb…`；真实同一 spy test 断言 write_count2 对 expected1，Cargo101；恢复 `a501…` 后测试1/0、exit0。remote authority5、本地persistent1、两个capability各1也通过。这些 Runner unit fixtures 使用明示 child-local bootstrap/feature，只计 DIAGNOSTIC。普通 parser3/0、全库fmt0/diff0另有真实 raw；未修改既有 coding_agent E0658。

最终两条 current copy manifest 的过时外部可用性备注已精确修正。旧错误 manifest 与原36e348冻结原字节保留；v2只改变 current manifest hash并加入两份历史文件，source/tests/raw及文档正文未变。83份现存复制证据逐哈希完好。用户磁盘清理删除的外部原始报告、native binaries、accepted-base scratch 明确 NOT_AVAILABLE，未伪造恢复。原生证据已在本次审查中直接检查过；当前意见依靠相同生产源码与保留 raw，不声称现在重新哈希不存在的 binary。

local fallback 仅 SOURCE_DERIVED_ONLY。Windows/Linux native、真实远端、八项restart、PID复用、whole-family/output continuity均未证明。setsid/FD反证仍是架构不变量 FAIL，封存实验原语未成为 production API。

提交尚未产生，必须另做 fresh exact-SHA 确认：父提交=基线、165项及v2清单实际入commit、三个被 *.patch 忽略的证据通过精确force-add入仓库，后续只允许另经核对的签署/Git metadata。这份意见不代替未知 commit 的审查。

本轴使用 code-review 与 specialized-domain-audit skills，自身无子代理、无仓库修改。检查脚本与详细 JSON 位于本恢复目录；执行者负责实际测试，本轴独立核对代码、退出 raw、哈希与证据等级，没有重复未变测试。
