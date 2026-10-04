# P1B exact-G 独立 Spec／安全／架构验收

**G_ACCEPTANCE = PASS_SCOPED。G 范围内未关闭阻断 finding 为 0。** 受审提交为 `8c84dee28d297c4a40d7269fe35531c56fa2eef7`。本结论接受已冻结的 P1B 实现、guard 与列明验证范围，允许随后执行严格 docs/evidence-only C；P1B 最终完成仍须 C 永久归档和本上下文的 fresh exact-C 审查。B/C、未运行平台和 Runner 测试目标限制不因此升级。

审查角色为独立 Spec／安全／架构验收上下文，未由 Luna 执行者自批。另一独立 Standards 的 exact-G 签署仍是独立轴；本报告对标准规则的核查不替代其角色。审查仅在工作目录产生报告、机器绑定及读取式核验脚本；没有修改 repo、index、生产、guard、测试、inventory，没有 commit/push，没有启动 P1C 或安装/切换工具链。

本文件设计归档到 `research/implementation/p1b/reviews/`；下面相对链接按该永久目录解析。未归档 working 来源只用字面路径标识。机器逐项绑定见同目录 [p1b-exact-g-bindings.json](p1b-exact-g-bindings.json)。

## 固定身份与实际差异

| 对象 | 实际核验身份 |
|---|---|
| 仓库／分支 | `/Users/songshiyao/Desktop/Projects/webcodex` ／ `impl/webcodex-p1b-normalization-slice1` |
| START | `36ed7dafad62dc05f79a32f236d395df2ab64be7`，为 G 的祖先 |
| S＝G 的直接父提交 | `5ae81148f3943982dfcd1814e1083f947b00a323` |
| G | `8c84dee28d297c4a40d7269fe35531c56fa2eef7`，审查开始和核验时本地 HEAD，干净工作区 |
| G tree | `3417c108c41509d67f74442097cecead3ce807a6` |
| S→G 35-path map SHA-256 | `f3af51939d15edcdbb92a363c250899d15a7229393a131643bd8247639937f7a`，sorted-key compact JSON，35 项 path 和 Git blob 字节全等冻结快照 |
| S→G 实际 binary diff SHA-256 | `8620849ac1316962a537637988f63def48621317e39e4fd1326ce03b99773547`，与冻结 staged diff 相等 |
| 唯一 inventory | `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee` |
| guard | `47c8456b7822d6ceeefcea7e21e8caf4157b58e8c7ab6501a96eaf3f37b580cc` |
| G manifest | `242e92bf0b3fb23c21292f52c1aeddd85c4e05e2a633b3cfcefb58eec826ac2c` |
| attrs／workspace policy | `d7c188a57a3a656febba7ef7d3879c8f30b50932ebf5fd5c77c4e61c3ba93cae` ／ `5583f87a2b61a939ef7cc8d0e615b5dd8d202b7c614498750bb2750d9ca07f00` |

`git diff --check START G` 实际 exit 0；没有用空的 unstaged diff 替代新增文件检查。35 路径严格为 guard/native test gates、5 个 dev-dependency 引用及 lock、attrs、单项 workspace allowlist、永久 harness、唯一 inventory、证据和三个旧文档的当前权威导航。G 没有生产行为修改。START→S 的生产差异仅为已授权 LSP 和 registered shell preparation 迁移；checkpoint 改动只在 test 区域。

`git_broker.rs`、catalog、local execution、coding agent、process broker 和 workspace checker 与 START 源码相同。checkpoint 的 `#[cfg(test)]` 前 28395 字节，START／54fcc／S／G 实际 SHA 均为 `cae15803832ace75047a902167cd2676e78ed81925c93676de3c82e4ca1502e5`。没有重新归一化 durable 生产实现。

## 独立方法与计量

先读根 AGENTS、CONTRIBUTING 和测试规则，先写本上下文 `p1b-exact-g-cleanroom.md` 的十项判据，再读实现与旧审查结论。旧 pre-G／EOF／Standards 报告只作为定位和历史身份线索，不能替 G 签署。CodeGraph 实际 `explore` 失败为 `unable to open database file`；没有写入或重建索引，降级为固定 SHA 的 Git blob、文件及原证核查。

完整 JSON 程序重算：123 logical launches＝A4+B22+C9+D75+E13；35 model execution-authority launches＋4 fixed-D model-triggered probes＝39 model-triggered launches；84 operator/build-only。20 forwarding/infrastructure anchors 与 89 non-process anchors 不再加入 launch 数。另一个物理维度为 28 Rust targets、260 grouped raw rows／290 occurrences、30 references、10 body fingerprints、626 production origin sources、128 non-Rust release assets。

上述数量来自 [唯一 JSON](../launch-inventory.json)，没有从 manifest 的 PASS 字段或 Markdown 表格推断。G 的全部 626 origin、128 asset、96 source snapshot 和所有 inventory 行的来源 hash 已逐项核对；manifest 的 16 source、18 raw evidence 和 4 permanent harness 绑定均一致。

A 为四路 broker 路由。B22 的逐项 `next_contract` 明确保留 provider/application/browser/SSH/pre-registration/worktree/plugin 的 exact capability、credential recipient、network purpose 和 uncertain outcome；**其 confinement 仍为 NOT_PROVEN**。C9 保留 P1C durable/session 设计，含 Runner restart/reconciliation、FD7/8、process groups/breakaway、监督者、完整性与未知结果。四个 fixed-D 为 Node version、Python canonical-module、developer-dir 与 Git candidate 探测；模型触发不等于模型可选任意 payload。

## 十二项对抗问题：规则、实现和原证对应

| 问题 | 实现／原证 | 判定与限制 |
|---|---|---|
| 1. 是否还有模型触发的 broker 旁路？ | `local_execution.rs:309`；`shell.rs:1611,1708`；`git_broker.rs:556`；LSP `supervisor.rs:233` 和 Runner `lsp/adapter.rs:28`。guard 的 actual origin/reference 比较和复用 NC。 | **全局答案仍为有。** A 四路进入 broker；B22/C9 和四个 fixed-D 按真实权限分类保留。不能写 `all model bypass zero`。 |
| 2. 是否 fallback 到 HOME 或更宽 root？ | `sandbox_authority.rs:91` 只解析 registry；shell cache hit 前重验；LSP adapter 从 exact project id 解析并 canonicalize；catalog `:432` 的 raw path 仅展示。 | **受审 A fail closed。** 缺 registry、失效 root、不匹配项目不裸启动。B 的 host/multi-root 权限继续具名，不从 cwd 推导。 |
| 3. 任意宿主 secret 是否可泄漏？ | LSP `supervisor.rs:35` exact locale list；local execution `:355` sanitize；shell registered prepare 用 Minimal snapshot。旧 LC predicate NC 的未知 secret 断言 exit101；真实 shell old RED、新值 absent。 | **迁移 A 的 arbitrary host-env 继承被封堵。** 明确配置/profile 及 B 的 credential audience 不因此被删除；B 环境 confinement 未证明。 |
| 4. Deny profile 是否仍可联网？ | broker native case E `execution_broker.rs:704` 先由 unsandboxed `/usr/bin/nc` 连同一 listener，再要求 broker child started 且 denied；case C 要求两层 descendant started 且拒绝外读。 | **列明 macOS profile 实测通过。** 亲读 precondition 与禁止结果 assertion，当前 system nc 存在；原日志没有 BLOCKED[E]。B 合法 network purpose 不改称 Deny。Linux/Windows native NOT_RUN。 |
| 5. deadline 会否被延长？ | bounded Git `:273` 在 probe 前设 absolute deadline；`probe_deadline:506` 取 caller/internal min；`open_drain_tail:913` 只开一次；guard metadata `:865–981` 共享 30s，4MiB/1MiB caps。 | **受审已有预算路径通过。** catalog real fsmonitor 返回约2.267s、dirty=null；held-pipe explicit 约0.16s。无 timeout 的 checkpoint 私有调用不被虚构为延长 caller deadline。 |
| 6. descendant 是否超出宣称生命周期存活？ | 真实 shell harness 在 Runner 尚存活和 operation 返回时检查 owned descendant 消失；LSP real-process lane含 leader exit、shutdown/drop、shared deadline；native C 证明 profile 继承。 | **受审普通 A ownership 通过。** 不承诺任意逃逸树、external attach、远端树或 C durable child 都立即消失；held-pipe 只证明返回预算。 |
| 7. incomplete 是否会被当 complete？ | catalog `:294–338` 拒绝非成功、cap、timed_out；Git 将 drain_incomplete 折入 timed_out。fsmonitor raw 的 hook 确实运行且 dirty=null；checkpoint feature-on disk bytes35 后才 PASS。 | **受审 consumer 通过。** inventory page complete 是分页状态，不能解释为每项 Git metadata complete。默认 feature-off workspace suite不能替代 checkpoint native。 |
| 8. ENV_BLOCKED 是否可 cargo-green？ | broker host gate `:146` panic；normalization `:252` 必须 Pass；checkpoint test `:999,1055,1075` panic。实际 old ENV exit0/new exit101。永久 shell/catalog actual main/probe 已在本上下文执行。 | **本次受审 native gates 通过。** 只有 PASS 返回0；ENV/HOST/FAIL/RED、assertion、timeout、unknown permission 非成功。should_panic/accounting 是退出传播证明，不能当 native confinement。其他未执行兼容性 probe 不计成功。 |
| 9. guard 是否有简单绕过？ | `p1b_guard.rs` 的 metadata/module/cfg/path/alias/macro、actual raw/reference/origin/asset comparator；compiled helper reuse、native alias、new/renamed boundary caller、实际 npm publication NC。 | **当前 exact inventory 边界通过。** 新 caller即使无新原语也触发 origin RED；macro未知 fail closed；npm mjs/cjs/无后缀均 published→RED→restore。门为保守全源 review gate，不是语义调用图，也不抵抗审核者同时主动改 guard+inventory。inventory SHA由本独立审查核验，guard没有硬编码 d5 assertion。 |
| 10. 是否把 P1C 普通化为 ManagedChild？ | JSON C9 的精确 next contracts；START→G 完整路径和生产字节核查。 | **没有。** persistent session、detached payload/supervisor/watchdog和remote session生产源未因本轮改动；P1C 未开始。 |
| 11. 是否改变业务语义？ | LSP 保留 executable/argv0、relative PATH、profile env precedence、piped I/O；shell复用原 collector、scope区分、authority-before-cache。真实 RunShell stdout39/stderr17、EOF、PATH、注销后未启动。 | **授权迁移内通过。** 安全拒绝为预期变化；TrustedProvider B 的原配置权限保持。40 default／37 explicit LSP 和真实 shell/catalog支撑列明行为；Runner unit未运行，不宣称所有业务回归均通过。 |
| 12. 是否有无关扩散？ | G全部35 paths／START→G hunks、checkpoint prefix、未变 coding_agent/checkers；dev-deps仅测试；workspace policy仅补现存 normal edge。 | **没有发现无关生产改动。** attrs用于 inventory 全源字节稳定，EOF豁免只直属 evidence `.log`；旧报告保留，新导航不覆写旧历史。 |

以上所需源码均在 G 的实际 Git blob 内检查，相关 anchor 和 SHA 位于机器绑定。新缺陷才需新最小 probe，本轮没有发现需要重新启动高成本 native suite的候选缺陷。

## 验证事实与标准边界

[default guard 原证](../evidence/p1b-guard-default.log) 实际为15 passed／2 ignored，约22.47s。两个 ignored 均显式执行：[npm pack](../evidence/npm-pack-closure-explicit.log) 1 passed和[held-pipe](../evidence/held-grandchild-deadline-current.log) 1 passed。[compiled reuse](../evidence/compiled-origin-reuse-corrected.log)、[native alias](../evidence/native-api-alias-negative-control.log) 和 [boundary caller](../evidence/boundary-caller-negative-control.log) 都有 actual comparator RED 与恢复原证，不仅是手工 expected 标签。共置 tests/comments/formatting 也会触发全源门的维护成本已在 JSON 和可读库存公开。

[native broker 五例](../evidence/execution-broker-native-five.log) 的 A–E断言已亲读；[catalog raw](../evidence/runner-catalog-native.log) 有实际 HTTP inventory、clean/dirty/status-timeout 三例；[shell raw](../evidence/shell-profile-native.log) 有 baseline真实外写/宿主值 RED和candidate拒绝外写、值 absent、EOF、定量 stdout/stderr、operation-return descendant gone、注销后 prepare count不增加。不是依据总 PASS 字段推断 native 事实。

LSP四个最终 source hash与G相等；working `lsp-slice/review-fix/rawlogs/macos-cargo-test-lib-default-final.log` 和 `macos-cargo-test-lib-real-process-final.log` 分别为40和37，真实测试名覆盖wire/env/root/argv0/deadline/tree。working `newcheckpoint-gate-slice/native-positive.log` 明确 `workspace-checkpoints` feature-on，private `git_apply`写disk35字节后PASS；其同环境old/new nested原证为0／101。working `guard-slice/old-native-gate-overlay.log`／`new-native-gate.log` 也记录旧假绿和新失败。未来永久归档必须原bytes保留这些历史失败与旧HEAD。

本上下文实际执行两个永久 stdlib accounting probe，均exit0；其实际 main 中non-PASS exit1，普通错误/unknown permission不能漂成HOST/ENV成功。独立输出为 working `architecture/reviews/p1b-exact-g-catalog-accounting.log` 和 `p1b-exact-g-shell-accounting.log`，完整observations和hash收录机器绑定；不把注入当 native证据。

workspace policy的唯一变更是 `webcodex-workspace.normal` 增加原已存在 `webcodex-process`。START正式gate FAIL、current官方20 packages PASS／self-test16 PASS 的[原日志](../evidence/workspace-boundary-gate-current.log)和[reverse-layer NC](../evidence/workspace-policy-narrow-probe.log)已读；checker、layers和反向边拒绝没有放宽。

EOF规则只为 `research/implementation/p1b/evidence/*.log whitespace=-blank-at-eof`，其他 whitespace仍报错。七例独立原证与八例Standards原证的实际退出码均已核：9份Cargo raw旧规则2、新规则0；trailing-space、space-before-tab、范围外、nested、Rust或非log EOF对照仍2。所有raw字节保持。[checkout matrix](../evidence/checkout-autocrlf-matrix.json) 所引用两套真实 scratch clones，本上下文重新读取HEAD=S和local autocrlf=false/true，并逐项重算每套626 origins、128 assets、23auxiliary的working bytes、actual index blob与G blob，全部等matrix；没有凭matrix PASS字段推断。此证明不等于Windows native runtime。

## 二进制身份、缺口与后续 C

真实 candidate binary实际 SHA为 `d8350ab9c390843d82f902a1308b2f90e6372a17297bba09e2919be4a00710eb`，baseline为 `d88cbfc6da56e2c8eca0333e785b880797b15e8f93e8af9c8cb5bb6155713044`，两文件均存在且本上下文重算一致。working `guard-slice/integrated-head-evidence.manifest.json`（SHA `6a6fd3057ae18d9b4335cc265027b4bafa0f8994b646aa7b813265f2449e432b`）记录实际1.98.1 dogfood build命令、exit0、binary和source/raw hashes。

该 build记录生产head `54fcc318ea0ef44bac75590f95b0aae14b133414`，同时明确有guard/test/dev-dependency待提交脏字节。六个生产source在54fcc、S、G确实等hash；其记录的Cargo.lock与当前G相等，**与54fcc提交内旧lock不相等**。正确复用依据是生产source相等＋构建工作区lock绑定，不是“clean54fcc lock等G”。checkpoint只test区改变，production prefix未变。C须原样归档build identity record并清晰保留这一事实，不把START baseline dev build log冒称新dogfood build原证。

以下仍是明确限制：B22 confinement NOT_PROVEN；C9留P1C；Linux/Windows native runtime NOT_RUN；stable Runner unit/bin-test target既有 `coding_agent.rs:261` E0658/atomic_try_update导致测试体NOT_RUN，coding_agent source与START相同；ordinary Runner binary成功build/runtime不等于Runner unit通过；full CI/integration不在本次已证明范围。

Phase8永久归档在G仍未完成。本上下文核验 `architecture/reviews/root-phase8-raw-copy-map.json` 的**19个**实际来源SHA/bytes全部正确、G中19个目标均尚不存在。范围是合同的16份LSP/checkpoint/历史gap原证，加Runner E0658原log、七例EOF原JSON只更名`.log`、上述production build manifest只更名`.log`。这属于G后的预定闭环，不是必须改生产的G阻断finding。

**PERMIT_C = DOCS_EVIDENCE_ONLY**：在两独立exact-G轴通过后，执行者只可机械归档这19份原证、acceptedG报告和本机器绑定，更新P1B当前总结/接受Markdown及必要evidence provenance。复制不得trim、重写旧HEAD、删旧失败或把摘要伪装raw；source与target SHA须相等。provenance只填已知S/G身份，不hash自身，不写其包含提交自身C SHA。禁止改production、guard、harness、tests、Cargo/lock、attrs、workspacepolicy和唯一inventory d5。其他机器/源文件改变会使docs-only前提失效，须新G审查。

同一独立上下文必须fresh-review exact C：核验G祖先、完整`G..C`白名单、原bytes/provenance/source equality、永久relative links及12问文案的真实限制。C最终SHA仅记录仓库外closeout。完成这些后可接受限定P1B final YES，同时保留B/C/OS/test-target缺口；G本身不宣称最终全闭环、all-model-zero或all-CI-green。

## 十项判据与完成诚实说明

身份范围、唯一inventory分类、A授权路径、secret/network、单一预算、准确tree语义、退出码不假绿、实际结构门、窄标准修复九项在列明范围通过；第十项证据身份已通过，永久归档闭环明确待C。G阻断finding0，新的生产/guard修复要求0。标准对照没有新增documented violation或需要动作的smell；宽泛重构不属本次授权。

完整读取guard2923行、两个native harness及两个accounting probes、G manifest、相关diff、原native/guard/NC输出；inventory/matrix/bindings以完整程序逐行核验hash与计量，不声称人工通读23559行JSON。生产源码读相关路径、预算、env、scope、consumer与test assertion段；根AGENTS/CONTRIBUTING/TESTING完整，架构/权限/决策取相关规则段。旧报告部分读取为历史线索，未作为PASS依据；无关全仓模块未重读。

实际本轮新增执行为读取式身份/hash/index检查、两permanent accounting probes和START→G whitespace检查。未重复稳定native suite、未做Linux/Windows/full-CI执行；已有结果通过真实source和binary equality复用。使用skill为 `code-review`、`specialized-domain-audit`；没有新增子代理。规则/质量门禁/repo文档均未由本审查者改动，仅工作目录审查文档新增。历史报告保留。

**正式签署：exact G Spec／安全／架构 PASS_SCOPED；允许上述 C 机械归档，最终 P1B completion PENDING fresh exact C。**
