# 规格与安全独立审查

结论：**ACCEPT**。Findings：**0**。仅绑定以下未提交 overlay；不授权合并、部署或扩大工具面。

仓库：`/Users/songshiyao/Desktop/Projects/webcodex`；分支：`impl/webcodex-p1c-durable-execution`；base/HEAD：`c5ec0a89a5fed5bba943f636b7d5df7fccded3ba`。审查命令：`git diff c5ec0a89 -- crates/webcodex-chatgpt-safe/src/main.rs crates/webcodex-chatgpt-safe/tests/protocol.rs research/implementation/dogfood/OPERATIONS.md`。

Clean-room（先于 diff）：真实请求含 `name`、`arguments={}`、对象 `_meta`，旧入口将它误判为未知字段。最小理想方案只在外层识别、验证并丢弃 `_meta`；不清洗或合并业务参数。仍须 `name+arguments`，未知字段 fail closed。[官方 2025-03-26 schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-03-26/schema.ts) 的 `Request.params` 支持 `_meta`；通用协议可省略 `arguments`，本次授权明确保持必填。

核对：`main.rs:332-353` 与理想方案一致。metadata 只进入对象类型检查和外层 `exact_keys`，下游独立读取 `arguments`。`SAFE_TOOLS`/discovery 仍为八工具；每个业务 `exact_keys`、`check_project`、注册根、Minimal 环境、网络策略、broker 和 timeout 均未修改。root/env/network/project_id 无 metadata 注入路径。`serve/handle`、helper payload 与结果包装未回显或记录 metadata，64 KiB 请求上限仍覆盖它。OPERATIONS 新段准确。

测试审读：正例会在旧入口明确失败；负例检查畸形 metadata、未知外层键、业务内 `_meta`/root、缺失 arguments、隐藏工具和 metadata 冒充 project_id。没有放宽原断言或以 ENV_BLOCKED 算通过。本人测试执行 **NOT_RUN**；独立读取外部 native 原始记录：`protocol --include-ignored` 为 **8 passed/0 failed/0 ignored，rc=0**；doctor 为 ready/rc=0；旧 binary NC 显示普通请求成功、同业务参数加 `_meta` 为 -32602。新 binary 的真实 ChatGPT 端到端结果未由本审查证明。

CodeGraph explore 因 `unable to open database file` 不可用，已降级有界源码读取。只写本外部报告；未修改仓库、测试、commit、浏览器或密钥。使用 Skill：code-review、web-access；无新增子代理。

仓库 reviewed-file SHA-256：

| 文件（相对仓库） | SHA-256 |
|---|---|
| AGENTS.md | cfdaa1b9ac4df2a996895232505defd87a5dfadf7b0e78a3c083a7b2f663ae4e |
| docs/agent/tool-contract-guidelines.md | 75b20a83c139ffc026b48e5c6801166a8c6fd43dbcbc2421f08bdf6121c563f3 |
| crates/webcodex-chatgpt-safe/src/main.rs | 04974b16ebfefef9a17e0a0e59a762764d671daf323156a72deb8ba3c7ee1b8e |
| crates/webcodex-chatgpt-safe/tests/protocol.rs | 52203c26c31c3c438be9246369ff098c0a651e6ac443580de8bbb76d05be3b06 |
| research/implementation/dogfood/OPERATIONS.md | e9f46da9fac0f7d6e6528ba61453999c76bc4b09976f2223401e2e362d3bd48f |

已读取外部证据 SHA-256（相对此报告目录）：

| 文件 | SHA-256 |
|---|---|
| mcp-wire-shapes.jsonl | fb0e17871be7d04b5e9dba07b7b023efa40458dfba9465be653c9eaa62a7a45f |
| protocol-include-ignored.run.json | 39fcab7f67d32844ed3d29fa45c1f3485a3d08c65b19c073715d97a720745624 |
| protocol-include-ignored.stdout | 5cd7c7fe7c8edbe33eafca6726b629aef790a8988bd9ef1267b3f0093e90b6b3 |
| protocol-include-ignored.exit_code | 9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa |
| protocol-include-ignored.stderr | 046425486d3aeff59a630c08b51646cb263a907c0ce318b257e9b70259610b9e |
| metadata-negative-control.raw.json | beea133d74af242096819f5caef61ba738b420a4fd34a9b7b7339363eaffc86e |
| newbinary-doctor.stdout | 6b3ee1c8f852a1a100d1e0c11174b5659135bfb09411d7dd3b0467e9d290269f |
| newbinary-doctor.exit_code | 9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa |
| newbinary-doctor.run.json | d56a089b616666ef67f6e5d7a0c5f8a77ee2a6afffd6137c8dcb92087676b2ca |
| fixed-source-hashes.json | b867f737f92dc42e76db832edf2331f9817df42a848d360a539aa8b6c7b34204 |
