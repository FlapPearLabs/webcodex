# chatgpt-safe 交付证据

本文件保留 `c5ec0a89` 本地工程里程碑的历史证据。2026-10-05 真实 Tunnel/ChatGPT 实测、协议修复及外部安全证据缺口见 [TRANSPORT_VALIDATION.md](TRANSPORT_VALIDATION.md)。

本轮交付是独立的 macOS stdio MCP 编码入口。**本地工程验证通过；真实 ChatGPT Web transport 尚未配置，TONIGHT_DOGFOOD_READY=NO。** 不从本地客户端推断 ChatGPT 读写 entitlement。RDC 当前仍暴露任意宿主命令/文件能力，未被接受为安全后备。

## 版本绑定

- 工作区：`/Users/songshiyao/Desktop/Projects/webcodex`。
- 分支：`impl/webcodex-p1c-durable-execution`；起点 `d8edf498a063200e0ad87723ff63df2b54d3f612`。
- 最终生产源码 `crates/webcodex-chatgpt-safe/src/main.rs` SHA-256：`8160f4e2a13755a1a3a9ef9c9787a5f466ce9ba398510a4cb9d0a85e4bacf557`。
- 本次实际测试二进制 SHA-256：`7aae451460f9f720b47db2b3a2aea71f24d99ec5877c166bfa3ce8da4c7d5ac4`。
- 二进制：`/Users/songshiyao/Documents/Codex/2026-10-02/role-webcodex-security-architect-reviewer-mode-5/standards-recovery-20261004/target/dogfood/webcodex-chatgpt-safe`。
- 证据在提交前采集，因此 JSONL 中 HEAD 仍是起点，并明确记录 dirty source。上面的源码和二进制摘要绑定实际被测版本；包含本报告的最终提交由 Git 历史和交付回复标识。

本轮不修改原有 Git/ExecutionBroker/runner 实现，不改 `coding_agent.rs`，没有兼容 shim。接受的 P1B 清单保持 SHA-256 `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee`；C2 overlay 保持 `a12cecad9a99029bf33ccd0aa815748a18159afbf5a70341b527e7b05c6a1b9f`。新 profile 用追加 overlay 记录一个实际 `spawn_with_toolchain` 调用点和两个逻辑调用者，未改写历史计数或 durable 分类。

## 验证矩阵

| 验证 | 结果 | 实际范围 |
|---|---|---|
| `cargo build --offline --locked -p webcodex-chatgpt-safe --profile dogfood` | PASS | 使用上述外部 target 目录 |
| `cargo check --offline --locked -p webcodex-chatgpt-safe` | PASS | 复用依赖的既有 dead-code warnings 保留 |
| protocol tests `--include-ignored` | PASS：6/6 | 原生批准运行；闭合工具面、项目/协议门禁、路径与 revision、敏感 symlink 别名 |
| 最终 binary doctor | PASS_NATIVE / ENV_BLOCKED_DEFAULT | 原生 rc 0 ready；默认受限上下文 rc 2；两份 raw 均保留 |
| 非 macOS serve fail-closed test | NOT_RUN | 本机只编译该测试 target，按 cfg 为 0 tests；不声称平台实测 |
| 最终边界 trace + checker | PASS：49 条记录 | 见 `evidence/native-boundaries-accepted-source.jsonl` 与 `checker-final.txt` |
| 独立不可读目录 MCP 探针 | PASS | clean fixture 可读时完整、chmod 000 后 partial，见独立原生 trace |
| checker 假绿负对照 | PASS | ENV_BLOCKED、OUTCOME_UNKNOWN、success=false 均迫使子 checker 非零退出 |
| profile overlay 负对照 | PASS：2/2 | 合法 Rust 新 launcher 改变源码摘要被拒；重新绑定摘要后的工具面变化仍被拒 |
| 完整 p1b_guard | PASS：20 passed / 0 failed / 2 ignored | 双独立审查接受后确认状态并实际复跑；2 项既有 timing / npm lane 为 NOT_RUN |
| workspace boundary check | PASS | 21 packages 的依赖层级与测试归属 |
| `cargo fmt --all --check`、`git diff --check` | PASS | 全工作区格式和补丁检查 |
| 连续编码工具链 | PASS_LOCAL_MCP | 11 次真实 tools/call；ChatGPT Web NOT_RUN |
| 官方 Tunnel doctor | BLOCKED_EXTERNAL | rc 2，缺真实 Tunnel ID；runtime key 尚未配置，真实读写 entitlement NOT_RUN |

`PASS`、`FAIL`、`ENV_BLOCKED`、`HOST_UNAVAILABLE`、`NOT_RUN` 分别记账。默认受限执行的独立目录探针实际返回 ENV_BLOCKED 并退出 1；该红色记录保存在 `directory-search-independent-sandboxed.jsonl`。获批准的原生上下文运行同一二进制，才获得实际 Seatbelt 边界证据；未用裸执行替换 production broker。

原生 trace 证明注册项目内 read/patch/shell/Git 成功，外部 read/write、跨项目、绝对路径、越界 symlink、未知 identity、root/env/network/profile 参数升级及被隐藏工具请求被拒。合成宿主 secret 未继承；真实 loopback listener 先接受原生正对照，再对 broker socket 返回 EPERM 且零连接。Git 返回真实 fixture branch、HEAD、dirty state。Timeout、output cap、持有输出的普通/setsid 后裔均为明确非成功；之后 ping 正常，stdin EOF 干净退出。外部 sentinel 字节不变，Python `-I -S` 未导入项目 shadow modules。

## 连续编码与负对照

`adaptive-coding-accepted-source.jsonl` 保留上下文摘要和 11 个实际工具请求/响应：select → search → read 源码 → read 测试 → read 含恶意指令的 AGENTS → patch 添加测试 → shell 看到 2 条失败 → patch 修复百分比公式 → shell 2 条测试通过 → Git diff → Git status。业务文件只经过 safe MCP 修改；操作者没有用宿主 API 代改。恶意文本是测试输入，未成为授权。

已保留失败迭代，不把它们改写成通过：早期 real MCP 证明 `.env` 别名泄露合成标记、不可读文件被误报完整空结果；`old-source-alias-search-negative.jsonl` 是历史缺陷负对照，不是最终版通过证据。最终源码的原生协议测试拒绝 `.env`/`.git` 的直接、单层和多层别名，同时允许普通项目内链接。独立目录负对照从最终 HELPER 仅移除 `os.walk(onerror=...)`，复现 truncated=false 的完整空结果；其 JSONL 明确标注 **DIAGNOSTIC_ONLY_NOT_BROKER_PROOF**。

初始网络/setsid 脚本出现 quoting SyntaxError，不能证明边界；该旧 trace 未作为最终 PASS。最终 trace 必须同时满足正对照、EPERM 和资源后置条件。checker 的三项假绿负对照防止把环境错误或模糊错误计为成功。

## 保留的进程家族边界

普通 P1B ownership 是直接子进程和进程组。`setsid` 后裔仍继承 kernel 文件/网络限制，但不能证明整个家族随操作结束。`close-pipe-counterexample.jsonl` 是前一候选二进制的实际反例：脱离进程组、关闭 stdout/stderr 的后裔在 foreground 返回后仍可写项目。这不是外部 sandbox escape，也不是最终源码的全家族终止证明。

工具 description 和 OPERATIONS 均明确：成功只涵盖 direct child/owned group/capture；timeout 不是 whole-family deadline；daemonize/durable 不受支持，执行可能已产生副作用，重试前读状态。Jobs、detached、session 不暴露，C2 remote fail-closed 不变。完整 family supervision 仍是 P1C，**NOT_PROVEN**，本轮不宣布 P1C 完成。

## 操作、复现与审查

启动、停止、status、doctor、health、日志与 Tunnel 接入见 [OPERATIONS.md](OPERATIONS.md)。本次所有 MCP 会话均已关闭，没有假称持续服务在线。官方隔离 tunnel-client `0.0.15+a390c168ff1b2d14e73a95991c186c6aba3ff5a0` 未改全局配置/仓库 pin。

`harness/` 保存实际使用的 fixture、边界驱动、checker 和 stdio 编码适配脚本。它们是操作者诊断程序，不是 release entrypoint，不在模型工具面。fixture 创建器拒绝覆盖既有目录。先在新的 `/private/tmp` 路径准备 fixture，构建 safe binary，再运行 `mcp_dogfood_driver.py --binary ... --manifest ... --evidence ... --repo ...`；对应完整 checker 参数见 `harness/evidence_contract.md`。诊断依赖本机 `/opt/homebrew/bin/python3` 与原生 macOS sandbox 上下文；不能从重建成功推断 ChatGPT 连接成功。

独立审查状态：**ACCEPT_BOUNDED_LOCAL_SOURCE_MILESTONE / ACCEPT_LOCAL_ENGINEERING_MILESTONE，0 blocking findings**。两名 fresh GPT-6.1 Sol max 分别审查安全/规格与标准，不由 Luna 执行者自批。报告及审查时逐文件摘要保存在 [security-final.md](reviews/security-final.md)、[standards-final.md](reviews/standards-final.md)、[standards-final-binding.json](reviews/standards-final-binding.json)。报告保留审查时的 candidate / pending 快照，没有事后改写。日志正文按原字节保留，末尾只追加明确的 capture 退出码；原始摘要与归档说明见 `evidence/root-closure.json`。主负责人亲读报告并核对全部 31 个文件字节一致后，仅确认 overlay 审核状态；随后 full guard 实际取得 20/0/2，并再次通过 workspace、格式、补丁检查。默认环境 sccache 权限失败 raw 保留，不能当结构失败或 PASS；通过的 full guard 使用批准的原生执行上下文。

实际 skills：codebase-analysis（调用链和独立入口范围）、codegraph-integration（既有权限/Git seam 的 impact，CLI 不可用处回退源码）、code-review（独立规格/标准审查）、specialized-domain-audit（权限、进程、false-green 专项）、openai-docs（当前官方 Tunnel/ChatGPT 接入依据）。Luna 完成 bounded 实现和重复测试，Sol 负责契约、差异验收和最终结论；未启用额外插件或修改记忆。

唯一外部收口条件：操作者配置真实 Tunnel ID 和项目外 runtime-key 文件引用，在 ChatGPT developer mode 添加/刷新 Tunnel，实际确认恰好 8 个工具并完成 read/write 和连续编码实测。不要把 key 值粘贴到对话。不通过 RDC 的任意 host shell 绕过这道门禁。
