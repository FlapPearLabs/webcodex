# P1B exact G — 独立 Standards 审查

**Standards = PASS；documented findings 0；需动作 smell 0。** 此签署仅覆盖 G，不替 Spec/security，也不接受尚未审查的 C。审查者与前次冻结审查相同，使用 code-review、specialized-domain-audit，未派生代理。

此文供 C 原样归档到 `research/implementation/p1b/reviews/`；仓库相对链接按该永久目标目录解析。完整逐文件身份见同目录 [exact-G bindings](p1b-exact-G-standards-bindings.json)。

| 身份 | 实际值 |
|---|---|
| reviewed G | `8c84dee28d297c4a40d7269fe35531c56fa2eef7` |
| 唯一 parent / implementation source S | `5ae81148f3943982dfcd1814e1083f947b00a323` |
| fixed START | `36ed7dafad62dc05f79a32f236d395df2ab64be7` |
| G tree | `3417c108c41509d67f74442097cecead3ce807a6` |
| 35-path path→SHA map，JSON sort_keys/compact SHA-256 | `f3af51939d15edcdbb92a363c250899d15a7229393a131643bd8247639937f7a` |
| parent→G binary diff SHA-256 | `8620849ac1316962a537637988f63def48621317e39e4fd1326ce03b99773547` |
| [唯一 accepted inventory](../launch-inventory.json) SHA-256 | `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee` |
| [G manifest](../evidence/manifest.json) SHA-256 | `242e92bf0b3fb23c21292f52c1aeddd85c4e05e2a633b3cfcefb58eec826ac2c` |

实际 HEAD 与 branch、单 parent、START ancestry、完整 parent→G 35-path 集合均核验。每个 G blob 的实际 bytes/SHA 同独立冻结 map、root stage snapshot 和当时 clean 工作树相同；binary diff 与最后接受的 staged diff 完全同哈希。38 个 manifest 绑定、13 个稳定源码绑定逐项从 G objects 核验。protected git_broker/catalog/coding_agent 等于 START；checkpoint 生产 prefix 28395 bytes 等于 START，SHA-256 `cae15803832ace75047a902167cd2676e78ed81925c93676de3c82e4ca1502e5`。

AGENTS.md、CONTRIBUTING.md、docs/TESTING.md、适用架构与补充合同已读；前次完整 actual diff 和 guard 阅读、实现及测试证据仍有效，未发生源漂移。最小 dev-only 依赖、独立 discovery→comparison、process fixture I/O/deadline、外部工具 explicit lane、dependency policy 的单 allowlist 修复均保持此前 Standards 接受结论。Fowler smells 按 heuristic 判断，未要求泛重构。

S-GUARD-01 保持关闭：默认三后缀纯 overlay 实际比较 RED→恢复 PASS，真实 npm pack explicit 三后缀 inclusion→RED→恢复 PASS。独立默认 15 passed/2 ignored；npm explicit 首次 cache EPERM 失败原样保留，仅 child-local cache 重试 1 passed；held-pipe explicit 1 passed，仅证明 deadline return。

S-GUARD-02 保持关闭：[实际 checkout matrix](../evidence/checkout-autocrlf-matrix.json) 与当前 attrs SHA 绑定；false/true 实际 Git index checkout 各 626 origins、128 assets、23 辅助文件零漂移。前次 reviewer 新 clones 只读重核亦一致。workspace policy 的 exact START 既有 FAIL 已保留；批准的单 normal-dependency allowlist 修复后官方 gate PASS、自测 16/16，checker/layers 未改。

首次暂存九份 raw log 的 EOF 告警 exit 2 保留为历史；现有 `evidence/*.log` 仅取消 blank-at-eof，17 份原始日志字节不变。独立八例与 Sol 七例证明 trailing-space、space-before-tab、目录外/嵌套文件仍 exit 2。G attrs 已精确绑定；实际 `git diff --check S G` exit 0。原工作证据为 `architecture/reviews/staged-raw-log-whitespace-original.json` 和 `p1b-staged-log-whitespace-standards-probe.json`，它们是原始 source 路径，不冒充永久仓库目标。

有效的 macOS broker/catalog/shell 与 LSP/checkpoint 结果按实际 source/binary equality 复用，未重跑稳定 Cargo/native。Linux/Windows runtime NOT_RUN；Runner unit 既有 E0658 仍 NOT_RUN；不声称 full CI、merge-ready 或 all model bypass zero。此次未独立验证远端 push。

Phase 8 合同已读。后续 C 仅允许说明文档、G 报告机械归档、原始证据 byte-preserving 复制与必要 provenance；不得改 production/guard/harness/tests/Cargo/attrs/policy/JSON d5。C 需独立 fresh exact-C 审查；本报告不提前接受 C。审者仅写仓库外部审查 artifacts，无 repo/index 写入、commit 或 push。
