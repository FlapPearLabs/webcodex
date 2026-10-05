# Standards 独立审查

审查人：`/root/metadata_transport_standards_review`；独立于执行者。日期：2026-10-05。

范围：`/Users/songshiyao/Desktop/Projects/webcodex`，分支 `impl/webcodex-p1c-durable-execution`；已核验 base=HEAD=`c5ec0a89a5fed5bba943f636b7d5df7fccded3ba`，base..HEAD 提交列表为空。审查的是三文件未提交改动，命令：

```sh
git diff c5ec0a89a5fed5bba943f636b7d5df7fccded3ba -- crates/webcodex-chatgpt-safe/src/main.rs crates/webcodex-chatgpt-safe/tests/protocol.rs research/implementation/dogfood/OPERATIONS.md
```

## 审查快照

| 文件 | SHA-256 |
|---|---|
| `crates/webcodex-chatgpt-safe/src/main.rs` | `04974b16ebfefef9a17e0a0e59a762764d671daf323156a72deb8ba3c7ee1b8e` |
| `crates/webcodex-chatgpt-safe/tests/protocol.rs` | `52203c26c31c3c438be9246369ff098c0a651e6ac443580de8bbb76d05be3b06` |
| `research/implementation/dogfood/OPERATIONS.md` | `e9f46da9fac0f7d6e6528ba61453999c76bc4b09976f2223401e2e362d3bd48f` |

## Findings（少于 400 words）

**结论：NO_FINDINGS，限 Standards 源码审查。** 不构成运行、部署或业务最终验收。

**先独立设计、后读 diff：** ChatGPT 是具体消费者。仅接纳 `tools/call.params._meta` 对象；只丢弃协议元数据，不合并业务参数、不推导身份或权限。其余字段、工具准入和业务精确字段集保持闭合。

**文档规则违规：0。** 对照 `AGENTS.md` §§2–4、`CONTRIBUTING.md` Development workflow、`docs/agent/tool-contract-guidelines.md` §§1/5/10 和 `docs/TESTING.md` 的相关测试边界。`main.rs:338–350` 实现与独立设计一致：元数据只做形态检查，业务只克隆 `arguments`，未新增元数据回显或日志；64 KiB 输入上限仍适用。`OPERATIONS.md:25` 准确描述这一窄兼容边界。

**判断性 smells：0 个可操作 finding。** 已逐项应用完整 baseline：Mysterious Name、Duplicated Code、Feature Envy、Data Clumps、Primitive Obsession、Repeated Switches、Shotgun Surgery、Divergent Change、Speculative Generality、Message Chains、Middle Man、Refused Bequest。`exact_keys` 同时检查字段数量（`main.rs:454`），有/无 `_meta` 的两个调用保护不同完整字段集合，不应为去重改成宽松可选字段 helper。无新增 helper、配置、双重表示或无关重构。工具已强制的格式等不列为人工 finding。

**测试断言与负对照：** `protocol.rs:133–145` 分别断言确定、非空的项目列表和已选身份，消除双 null 假绿；`:146` 证明 metadata 中注册身份不能授权业务中的未注册项目；`:152–178` 覆盖 null/数组/各标量、额外外层字段、业务 `_meta`/root、缺失 arguments 和未准入工具。基线源码只容许两个外层字段，因此旧实现应拒绝 metadata 正例；这是静态对照，动态 negative-control 结果为 **NOT_RUN**。

## 验证与操作边界

`git diff --check`：exit 0。源码/测试/OPERATIONS 哈希复核一致；状态仍只有指定三文件修改。Cargo、Luna 测试结果与真实 ChatGPT transport：**NOT_RUN／未独立取得结果**，不得由本报告推断通过。

CodeGraph 已优先尝试只读 `explore`/`node`，均返回 `unable to open database file`；降级为限定文件读取，未重建索引。本审查只写此外部报告；未修改仓库、规则、门禁或测试，未提交、浏览、读取私密文件或 runtime key。
