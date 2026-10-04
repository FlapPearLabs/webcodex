# C2 Standards 最终审查：独立 cleanroom 模型

REVIEW_AXIS=STANDARDS
REVIEWER=独立 Sol Max Standards 上下文
REPO_MODE=READ_ONLY
BASE_SHA=f58e65c6d95bbd91165e97b4a98de694f97ae872
BRANCH=impl/webcodex-p1c-durable-execution
MODEL_CREATED_BEFORE_IMPLEMENTATION_DIFF=YES
OLD_STANDARDS_CONCLUSIONS_READ=NO
FORMAL_VERDICT=PENDING_FINAL_CANDIDATE_AND_EVIDENCE_FREEZE

## 问题与假设

完整原始请求要求 P1C 持久执行安全、生命周期与重启恢复，但本次已签署子契约仅关闭 remote C2 持久 shell 能力并退休其专用执行链。两个 C2 logical surface 必须保持原分类并记为 DEFERRED_FAIL_CLOSED；C7 为 ARCHITECTURE_DECISION_PENDING。不能将当前子契约完成推导为完整 P1C 完成，不能把退役/未运行的 Windows 测试推导为平台运行 PASS。S1 sealed 外部工作不构成此次候选代码的证据。

接受的 P1B 起点与 inventory 原始字节是不可变输入。现有 local PersistentShell、SshShell 和 B22 SSH 行为是需要保留的有效消费者；只清理本次关闭 remote durable contract 产生的 orphan。签署补充使最终专用测试退休集合恰为十一项，raw removals 恰为十项，reference removals 为零。

## 从零设计的最小实现

1. 在唯一公开 facade 获取 operation 和 ssh_resource 后，remote open/exec 在任何 config/credential/prepare/connect/spawn 前固定拒绝。固定 error code 是 remote_durable_authority_unavailable；信息直接说明后端不可用，不暴露伪造成功或恢复动作。
2. exec 的权威事实包含已有 transport 身份。若请求省略 ssh_resource，但 exact-owned summary 的 executor 为 ssh，仍在 output-limit 更新及 write/exec 前拒绝。请求标记不能取代 transport 身份。
3. remote exec 拒绝可以 exact-owned close 旧 transport；身份不匹配不能关闭其他 entry；cleanup 失败不得恢复执行。close/status 保持观察、幂等清理及 terminal 历史。close 只证明本地所持 transport 清理，不证明远端全族终止。
4. 若保留 direct private open_ssh/exec_ssh interface，则实现只能固定拒绝；旧不可达执行 body 也必须移除。固定拒绝可局部共享一个小 helper，不能为两个拒绝分支引入配置、策略层或通用 launch seam。
5. 从 capability 集合删除 SshPersistentShell，保留 SshShell 和 local PersistentShell。仅机械更新 manager constructor 的 orphan SSH pool 参数、唯一生产调用及相应测试 adapter。
6. 完整退休没有当前 local/B22 caller 的 remote_shell module 和 SSH 专用 preparer/availability、summary helper及十一项 fidelity/preparer tests。原始 body、原 SHA 和处置保留在独立历史档案中；不改为 skip-green 或虚报运行。
7. 面向消费者的 session 文档准确说明 remote backend unavailable 和已拥有 transport 的拒绝/清理；工程历史与范围账本放入 implementation evidence。解释 unavailable 的确切能力边界，不加入泛化失败恢复框架。

## 最小 guard interface 与 implementation

既有 AST/module scan 保持原样。accepted_inventory 只从 immutable P1B 加一个固定 C2 overlay得到，不重生成 accepted inventory，不从当前 source 自动采纳新 hash。

Overlay 的 interface 只有固定 schema/status/base SHA/base inventory hash、五项 exact origin replacements、一项 remote_shell origin removal、十项完整 raw removal keys与零 reference removal。每个 origin 带 exact path、旧 SHA、targets和已审查新 SHA；raw key带 file/symbol/primitive/count/targets。未知字段、错误 base/source SHA、重复、缺项、错 count/targets、不存在的 row或扩大集合均拒绝。拒绝发生在接受扫描差异前。无需通用 removal、目录跳过、状态升级开关或可变白名单。

内存应用只能改变所签署的 physical source origins/raw rows；logical surfaces/classification/counts、其余 targets/body/reference/non-Rust asset 完全保持。生产 gate 必须对真实 source mutation返回非零；单独 assert-error unit green不是同一证据。

## 可区分验证模型

- remote named open：合法与非法配置均固定拒绝，prepare/connect/spawn marker都不存在，active_count不增加；恢复旧真实 route/helpers在同源 scratch使相同行为断言非零 RED。
- remote named exec：owned spy write_count不变，exact cleanup幂等，unrelated identity保持；direct private helper调用也拒绝。
- omitted marker：合法 local cwd/profile加 summary.executor=ssh仍拒绝；独立去除该 fence后 spy新增写入使测试非零 RED。
- close/status与local/B22：存活行为的focused regressions；保留terminal result；不得以transport cleanup覆盖远端全族证明缺口。
- capability：真正supported_capabilities集合缺SshPersistentShell而保留SshShell/local PersistentShell。
- overlay：固定正常候选在授权签署状态下与scanner相合；unknown fields、wrong counts/targets、duplicate/missing origins/rows、classification/status/base变更拒绝；production launcher/退役primitive复活使同一cargo gate非零 RED，exact restore后回归。
- history/accounting：base bytes/hash不变，十一项完整历史test bodies可核对，十项raw/零reference差异精确对应源；C9账本明确0 migrated/2 deferred-fail-closed/7 pending。
- 跨平台：pure路径/encoding断言不冒充Windows native；只运行macOS给出实际平台与exit；ENV_BLOCKED、HOST_UNAVAILABLE、NOT_RUN不计PASS。

## Standards 与读深度计划

硬规则来自 repo AGENTS.md 的最小当前需求、orphan退休、外科手术范围、focused validation、dedicated test tree和真实边界；CONTRIBUTING.md 的focused change/consumer docs/trust-boundary evidence；docs/TESTING.md 的当前消费者、退休专用fixtures、无环境全局泄漏、EOF/capture、绝对deadline、失败不可ignore及真实process显式lane。code-review smell baseline只作为判断提示，不把个人架构偏好当blocker，也不重复格式/compiler已有门禁。

先read-only CodeGraph explore/impact/callers/callees/affected，再核读每个生产hunk、private/helper调用上下文、guard parser全body、完整新增测试和业务doc hunk。最终source hashes、commands、run counts/exit/read depth/WHY/source和standards links全部独立绑定；新source变动必须重新核对受影响部分。

SKILLS_USED=code-review,codebase-design,codegraph-integration
MEMORY_USE=仅采用独立审查/执行者green不构成验收的通用流程；未读取历史C2 Standards结论。
