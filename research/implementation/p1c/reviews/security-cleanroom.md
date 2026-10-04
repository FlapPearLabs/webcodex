# C2 最终 Spec/security 独立 cleanroom

角色：全新独立 Sol Spec/security 审查上下文。创建时尚未查看实现 diff 或实现函数；只读仓库，外部输出。完整 P1C 原始用户规格已读取。预期 base/当前 HEAD：`f58e65c6d95bbd91165e97b4a98de694f97ae872`。正式最终 verdict 等待主负责人提供 final manifest，再做独立源和 exact SHA 核验。

## 最小 C2 模型

允许提交的最小行为是关闭两个既有 remote persistent shell logical surface 的执行入口，直到存在已签署的远程 durable authority backend。它不需要新长期 supervisor、通用 launcher、恢复权限或本地 C7 架构。C2 状态只可为 `DEFERRED_FAIL_CLOSED`；其余七项保持 `ARCHITECTURE_DECISION_PENDING`；完整 `P1C_COMPLETE=NO`。

信任链应为：已授权普通 dispatch → persistent-shell facade → 在任何 SSH prepare/connect/credential materialization 或 transport write 前拒绝。模型传来的 `ssh_resource` 是路由提示，不能是已有 transport 的可信身份；省略标记的 exec 必须另核对既有 transport 的 `executor`。有标记的 open/exec 和直接 private remote helper 都返回固定 `remote_durable_authority_unavailable`。已有 remote exec 的 cleanup 只能关闭 exact owned workflow-session/runtime-project/shell identity，错误也不能恢复写入。close/status 只能沿当前观察及强身份绑定 cleanup，不从本地 transport close 推断远端全族终止。

删除仅包括 remote capability 广告、remote 执行体及因此产生的专用孤儿链。存活 local persistent shell、B22 one-shot/background SSH、普通 P1B broker 和 accepted P1B 历史 inventory 必须继续有原有边界和覆盖。十一项 whole dedicated fidelity/preparer 测试有明确签署；Windows 混合测试的 local Bash override 半段属于存活行为，不能用 remote retirement 抹去。保留 local 半段、只退休 remote missing-resource 半段是当前需求的最小处置；最终仍需独立核对实际 diff 和账本，不能额外批准 whole retirement。

Guard 应继续在既有 real-production AST/target/native/body/asset 扫描上比较 exact source。immutable P1B JSON 原字节 hash 固定 `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee`。versioned overlay 只在内存逐项应用：恰好五项 origins replacement（main.rs、mod.rs、persistent_shell.rs、ssh.rs、transport.rs），唯一 remote_shell.rs origin removal，完整键签署的十项 raw removal／零项 reference removal。旧 hash、targets、path、base SHA/hash、schema/status 都必须验证；未知字段、缺项、重复、额外来源、不存在或非唯一行一律拒绝。不允许扫描当前树后自动采纳新 hash，也不通过扩大匹配/exemption 容忍漂移。

## 独立验收判据

| 规则 | 最小证据 | 不能据此声称 |
|---|---|---|
| 有标记 remote open 在 prepare 前关闭 | 合法 project/resource 的实际 spy ssh／prepare/connect marker 为零；错误配置也无前置 materialization | 已实现 remote durable sandbox/backend |
| 有标记 exec 不写已有 transport | exact owned spy write=0；同 ID cleanup 幂等；其他 identity 保留 | remote payload 全族被杀 |
| 省略标记不能绕过 | 合法 cwd/profile + 既有 executor=ssh + omitted marker；write=0；独立移除该 fence 后同断言 exit 非零 | request 参数等于可信 transport identity |
| private helper 本身关闭 | 直接调用只返回固定错误，无 prepare/connect/write | 仅 handler 的 guard 足以覆盖所有调用 |
| capability 与 local/B22 保真 | 实际 supported_capabilities 构造只退 SSH persistent；local smoke／保留的专用边界测试；必要 B22 回归 | 全部 P1B 或 C7 runtime PASS |
| 退休范围精确 | frozen diff 与十一 whole + 一 partial test ledger；完整十 raw/零 reference；逐键对照旧源/immutable inventory | 任意额外 orphan prune 已获授权 |
| anti-bypass 对真实生产源关闭 | 正向完整 gate；无效 overlay 拒绝；fresh 同源 scratch 插真实 launcher或复活 remote primitive，使同一 Cargo production gate actual exit 非零，restore 后回绿 | assert-error 的绿色 unit harness 等于实际 RED |
| 原生及源绑定可靠 | exact source hashes／normal build toolchain／raw command、stdout、stderr、platform、exit；旧 route+helper 在同一 marker断言下 actual harness exit=1 | SOURCE_DERIVED_ONLY、bootstrap diagnostic、Windows HOST_UNAVAILABLE 为 native PASS |
| evidence 自洽并诚实 | manifest 每条可回溯 raw bytes与 source；失败迭代保留；所有 PASS 均有实际执行与非零负控 | C2 milestone 等于完整 P1C acceptance |

## 已读真源与独立方法

完整读取：原始 P1C 用户规格；repo AGENTS.md、CONTRIBUTING.md、docs/TESTING.md；code-review 和 specialized-domain-audit SKILL.md；REMOTE-C2-IMPLEMENTATION-FREEZE.md；C2-GUARD-OVERLAY-CONTRACT.md/V2.md；C2-SIGNED-RETIREMENT-ADDENDUM.md；SUPERVISION-DECISION.md；docs/agent/permission-model.md。architecture-decisions.md/README.md 初读输出截断，后续按相关章节补读，不宣称全读。旧 WebCodex 记忆仅帮助识别 false-green 风险，最终事实必须以本轮冻结源及实际证据为准。

后续以规则／实现／测试三层映射审查；正式 finding 给出 evidence、reproduction、actual、expected、risk、minimum fix。不修改 source、不改 overlay review 状态、不提交、不推送，不替用户选择未定长期边界。

状态：`CLEANROOM_READY / FINAL_MANIFEST_PENDING`。
