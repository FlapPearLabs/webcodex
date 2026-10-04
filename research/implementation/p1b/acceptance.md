# P1B 范围验收与证据索引

## 验收状态

当前实现与验证范围绑定到已审查的 G：`8c84dee28d297c4a40d7269fe35531c56fa2eef7`。
G 的直接父提交 S（实现与分类来源）为 `5ae81148f3943982dfcd1814e1083f947b00a323`；固定 START 为 `36ed7dafad62dc05f79a32f236d395df2ab64be7`。
本页记载 G 范围，不声称本页或 C 已获独立签署。

[Spec／安全／架构审查](reviews/p1b-exact-g-spec-security-acceptance.md) 对 G 作出 `PASS_SCOPED`，G 范围未关闭阻断 finding 为 0，并许可严格的 docs/evidence-only C。
[独立 Standards 审查](reviews/p1b-exact-G-standards-review.md) 对同一 G 作出 `PASS`，documented findings 与需动作 smell 均为 0。
审查绑定文件分别列出确切对象、hash 与限制。

本页记录 G 验收与 C 机械归档合同。
C 的确切 SHA 与独立签署由仓库外 exact-SHA closeout 核验；本页不自报 C 签署。
manifest 只绑定已知 S/G 身份及实际证据，不记录自身 hash 或未知 C SHA。

## 分类范围

唯一分类真源为 [launch-inventory.json](launch-inventory.json)，本次保持其字节与 SHA 不变：`d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee`。
以下数量来自该清单及 G 审查的逐项核验：

| 维度 | 数量／状态 |
|---|---|
| Rust production targets；grouped raw rows / occurrences | 28；260 / 290 |
| references；body fingerprints | 30；10 |
| production origin sources；non-Rust release assets | 626；128 |
| logical launch 分类 | A4 / B22 / C9 / D75 / E13，共 123 |
| model execution-authority；fixed-D model-triggered probes | 35；4，合计 model-triggered 39 |
| operator/build-only；infrastructure anchors | 84；20，后者不重复计入 logical launches |
| non-process anchors；unclassified | 89；0，不重复计入 logical launches |

A 的四路都进入普通 execution broker。
B 的 22 路仍是 provider/MCP/ACP/native app/browser/SSH/project creation/worktree/TypeScript plugin 等各自具名的权限和生命周期边界；其 confinement 为 `NOT_PROVEN`，须按精确 `next_contract` 后续评估，不能自动降格成普通 broker child。
C 的 9 路保留 P1C 合同，包括 FD7/8、process group 与 breakaway、监督和 reconciliation、Runner restart、orphan/incomplete 与 `OutcomeUnknown`。
P1C 尚未启动；也不把所有 B 路径强塞进 P1C。

Guard 的 production-origin closure 是保守的完整源变更门，会对新增或变更的 production source 要求复审；它不是语义调用图，也不是对任意同时篡改 guard 与 inventory 的防护。
旧 LSP LC 前缀、最初 36/1 失败、checkpoint 旧 false-green、旧 guard 缺口等历史失败以原始日志保存，不重写历史。

## 关键验证证据

下表中的永久 raw 由 G 后原样归档；命令输出、旧 HEAD、失败退出码和尾部空白均不改写。
文件后缀变化不改变源 bytes。

| 范围 | 实际结果及原证 |
|---|---|
| LSP 默认与 native lane | macOS 默认 40 passed；显式 `real-process-tests` lane 37 passed：[default](evidence/lsp-default-final.log)、[native](evidence/lsp-native-real-process-final.log)。不是 Linux/Windows 运行时结论。 |
| LSP 环境策略 | 正向 predicate 1 passed；恢复旧 `starts_with("LC_")` 后未知 LC secret 泄漏断言 exit 101：[positive](evidence/lsp-lc-policy-positive.log)、[旧策略 RED](evidence/historical-lsp-old-lc-prefix-red.log)、[旧源码 hash](evidence/historical-lsp-old-lc-prefix-source-hashes.log)。最终源码 hash 见 [source list](evidence/lsp-final-source-hashes.log)。 |
| LSP 早期 native lane | 初始 36/1 失败原样保留；未提供第二/第三项目 executable 的历史事实不改成 PASS：[日志](evidence/historical-lsp-native-initial-36-1.log)。 |
| checkpoint private patch | feature-on native patch test 在确认磁盘 35 字节后 PASS：[日志](evidence/checkpoint-native-positive.log)。feature-off/default suite 不替代该结果。 |
| checkpoint exit accounting | 旧 nested `ENV_BLOCKED` false-green exit 0；修复后同类 nested `ENV_BLOCKED` exit 101：[旧记录](evidence/historical-checkpoint-old-nested-green.log)、[修复记录](evidence/checkpoint-new-nested-red.log)、[源码 hash](evidence/checkpoint-source-hashes.log)。 |
| broker native gate | 旧 host gate 的 false-green 及修复后不可假绿的 raw 均保留：[旧](evidence/historical-execution-broker-old-native-gate-green.log)、[新](evidence/execution-broker-new-native-gate-red.log)。 |
| guard default lane | macOS 默认 15 passed、2 ignored；两个 ignored 项各有独立显式 1 passed 记录：[默认](evidence/p1b-guard-default.log)、[npm](evidence/npm-pack-closure-explicit.log)、[held pipe](evidence/held-grandchild-deadline-current.log)。held-pipe 只证明 deadline 能返回，不证明整棵进程树清理。 |
| guard 历史缺口 | 编译后 D helper 复用、native alias、npm mjs inclusion 的旧缺口探针原样保存：[D reuse](evidence/historical-de-origin-reuse-old-gap.log)、[alias](evidence/historical-native-alias-old-gap.log)、[npm mjs](evidence/historical-npm-mjs-old-gap.log)。最终 source-origin、alias、boundary 与 npm pack 的 corrected 证据仍以原 manifest 为准。 |
| Runner unit target | 稳定 Runner unit/bin-test target 因 `coding_agent.rs:261` 的 E0658 / `AtomicU64::try_update` 编译限制而未运行，原始日志：[记录](evidence/runner-test-target-e0658.log)。相关 `coding_agent.rs` 与 START 相同；这不是 PASS。 |
| EOF whitespace gate | 原始 probe JSON 仅改名为 `.log`、bytes 保持；其中直属 `evidence/*.log` 的窄规则与反例结果见[原证](evidence/raw-evidence-eof-whitespace-probe.log)。其他 whitespace 仍是门禁。 |
| Runner binary 与真实 harness | Rust 1.98.1 已有工具链构建的 candidate binary SHA 为 `d8350ab9c390843d82f902a1308b2f90e6372a17297bba09e2919be4a00710eb`，记录见[构建 manifest 原件](evidence/runner-production-build-manifest.log)。六个生产源码与 G/S/54fcc 相同且锁文件绑定到当前 G，但 build worktree 的 Cargo.lock 与 54fcc 提交内旧 lock 不同；这是源码相等下复用，不是 clean-G build。 |
| Runner catalog / shell native | 已有 macOS 实际 Runner→catalog→broker→Seatbelt→Git 证据：生产 Git 命令为 `rev-parse --abbrev-ref HEAD`、`log -1 --pretty=format:%h`、`status --short`；clean、dirty、timeout 是三种 fixture，timeout 返回 dirty=null（约 2.267 秒）。shell 有外写与宿主 secret 拒绝，以及真实 RunShell stdout 39、stderr 17、EOF、operation return 后 child gone 而 Runner 存活。详见 [catalog](evidence/runner-catalog-native.log) 与 [shell](evidence/shell-profile-native.log)。native 撤销 registry 绑定后拒绝执行；该 case 未证明 cache hit，cache hit 前授权顺序由 source 检查。 |
| Native broker | 五个 macOS Seatbelt case：workspace read/write 与 external read、descendant、per-action、network 断言，详见 [raw](evidence/execution-broker-native-five.log)。系统 `nc` 先确认 listener 可达，再观察 deny；实际 main classifier 对非 PASS 返回非零。未知环境/permission、普通失败、assertion 和 timeout 不计成功。 |
| Workspace policy | 官方 20 packages gate PASS、自测 16 PASS；START 的既有 normal dependency 漏项为 exit 1；候选窄 allowlist 与反向层级负控均有原证。见 [gate](evidence/workspace-boundary-gate-current.log)、[self-test](evidence/workspace-boundary-selftest-current.log)、[baseline](evidence/workspace-boundary-start-baseline.log)、[negative control](evidence/workspace-policy-narrow-probe.log)。 |
| 格式、差异与 checkout | G 的 fmt check、`git diff --check` 及 false/true Git checkout matrix 由现存 manifest / review bindings 记录；checkout 两种配置均核对 626 origins、128 assets。它是 Git 字节复现证据，不是 Windows native runtime。 |

## 12 项对抗问题的范围结论

| # | G 范围判定 |
|---:|---|
| 1 | 全局仍存在模型触发 authority；A 四路经 broker，B22/C9 与四个 fixed-D 按其真实权限保留。不能声称 `all model bypass zero`。 |
| 2 | 受审 A 的 root 来自 registry/project authority，缺失或无效 authority fail closed；B 的 host/multi-root authority 不由 cwd 推导。 |
| 3 | LSP exact locale allowlist、local execution sanitization 与 shell Minimal snapshot 限制受审 A 的任意宿主环境继承；B 的 credential audience 和 confinement 仍须单独证明。 |
| 4 | 当前 macOS Deny profile 的网络拒绝有先行 listener precondition 和 native assertions；Linux/Windows native 未运行。B 合法网络权限不改称 Deny。 |
| 5 | Git probe 使用 caller/internal deadline 的较小绝对预算；held-pipe 只证明 deadline 返回；不把无 timeout 的 checkpoint private call 描述为延长 caller deadline。 |
| 6 | 普通 A ownership 的真实 shell 子进程在 Runner 存活和 operation 返回后已观察到结束；不承诺 detached、远端或 C durable child 立即消失。 |
| 7 | 不完整的 Git/status 结果保留为 incomplete/null；feature-on checkpoint 在 35 字节检查后才通过；分页完整性不等于每项 metadata 完整。 |
| 8 | 受审 native gate 将 `PASS` 作为唯一成功；旧 ENV false-green 与修复后 exit 101 原证保留。should-panic/accounting 只证明退出传播，不充当 native confinement。 |
| 9 | exact inventory 边界下 compiled helper reuse、native alias、boundary caller 与 npm publication 均有真实负控；source closure 保守且不是语义调用图。 |
| 10 | C9 仍属于持久 session / detached supervisor 的专属生命周期问题；生产持久化边界未迁移，P1C 未启动。 |
| 11 | 已证明授权迁移范围内 executable/argv0、相对 PATH、profile env precedence、piped I/O、scope 和 authority-before-cache 行为；Runner unit 未运行，不能概括所有业务回归通过。 |
| 12 | G 仅包含已审的 35 路径；production broker/catalog/coding-agent 等与 START 源码相同，checkpoint production prefix 相同。C 不扩散到生产、guard、harness、tests 或唯一 JSON inventory。 |

## 明确保留的边界与后续

P1B 是 `PASS_SCOPED`，不是全平台或全 authority 面完成。
B22 confinement 为 `NOT_PROVEN`；C9 留待尚未启动的 P1C durable contract；Linux/Windows native runtime 与 Runner unit target 未运行；full CI 未运行。
历史 checkout matrix 不等于 Windows native 验证。
macOS native harness 使用临时项目和 loopback 服务，没有联系外部生产主机。

P1B branch 上 G 已推送；main 的 `7301186..` 为 G 的祖先关系证据。
完整 PR/CI 未运行，尚未 merge。
后续仍需正常 PR 与 required CI 集成；本次 C 不创建 PR、不合并、不部署。

Rust Homebrew 1.94.0 和已有 rustup 1.98.1 按普通用途复用；本轮未安装工具链、改 default、改全局配置或重跑 Cargo/native suite。
六个生产源码 hash equality 支持已有 Runner binary/harness 复用，不把 build worktree 描述为 G 的 clean build。

## C 的复核合同

G 的 Spec／安全／架构与 Standards 两轴各自签署 G；这些报告不替代 C 的独立验收。
本页记录 G 验收与 C 机械归档合同；外部 exact-C closeout 独立核验 G..C 完整路径白名单、bytes、provenance、链接与范围，并记录确切 C SHA 和签署。本页不自报 C 签署或记录 manifest 自身 SHA。
