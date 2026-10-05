# ChatGPT 真实 transport 验证（2026-10-05）

**真实读写和连续编码通过；严格远端安全总门禁仍有一个外部证据缺口。TONIGHT_DOGFOOD_READY=NO。** 项目外 patch 请求被 OpenAI 工具安全层提前屏蔽，未到 WebCodex；不能把这一拒绝算作 WebCodex 边界证明，也不能从中推断账户套餐或 write entitlement 不可用。

## 版本与范围

起点为 `c5ec0a89a5fed5bba943f636b7d5df7fccded3ba`，分支 `impl/webcodex-p1c-durable-execution`。被测 `main.rs` SHA-256 为 `04974b16ebfefef9a17e0a0e59a762764d671daf323156a72deb8ba3c7ee1b8e`，dogfood binary SHA-256 为 `ab366914666171140acd8031efb35c263185e813378e4d7e0f16262716907ba9`。证据在提交前采集；包含本报告的提交由 Git 历史绑定，最终 exact SHA 在交付状态中记录。

此次只修 `tools/call` 外层协议校验。真实 ChatGPT 请求包含 `name`、`arguments`、对象 `_meta`；旧二进制拒绝 `_meta` 并返回 -32602。新入口接纳对象 metadata 后忽略它，不合并业务参数、不推导项目权限；业务精确字段、八工具、broker、环境、网络、deadline 均不变。缺少 arguments、畸形 metadata、未知 outer/业务字段和隐藏工具仍失败。官方 [MCP schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-03-26/schema.ts) 定义了外层 metadata。通用协议允许省略 arguments，本次未扩张该既有入口契约；实测 ChatGPT 传入 `{}`。

## 真实接入与编码

官方 tunnel-client `0.0.15+a390c168ff1b2d14e73a95991c186c6aba3ff5a0` 使用 `webcodex-chatgpt` profile 和 stdio。runtime key 在仓库外权限受限文件中，配置只引用 `file:`。无公开 WebCodex listener，未安装可选 Codex Tunnel 插件，也未接入 RDC。

CLI doctor rc=0，但其 key PASS 仅表示已配置，stdio reachability 为 SKIP。控制面实际取得 tunnel metadata、ChatGPT 实际调用以及 live/ready HTTP 200 才共同证明 auth、可达和 ready。ChatGPT 显示已连接，发现恰好八工具（Read 6 / Write 2）：`project_list`、`project_select`、`files_search`、`files_read`、`files_apply_patch`、`shell_run`、`git_status`、`git_diff`。

唯一注册 disposable 项目为 `/private/tmp/webcodex-chatgpt-remote-20261005/project`，id `dogfood-project`。只读链路实际成功。搜索明确返回 `truncated=true`（fixture 含越界链接），不声称完整扫描；源码读取完整。

一次 ChatGPT 用户任务连续执行 12 次工具调用，没有搬运中间结果，也未由宿主接口代改业务文件：

```
project_list → project_select → files_search
→ files_read(pricing.py) → files_read(test_pricing.py) → files_read(AGENTS.md)
→ shell_run(unittest: 1 test failed)
→ files_apply_patch(pricing.py) → files_apply_patch(test_pricing.py)
→ shell_run(unittest: 2 tests passed) → git_diff → git_status
```

修复由 `price - percent` 变为 `price * (1 - percent / 100)`，新增 `discounted_price(80, 25) == 60` 回归。最终 shell rc=0，timeout/incomplete/stream truncation 均 false。真实 Git 状态为 master、HEAD 9efd637，两文件 modified，并如实保留 unittest 生成的 `__pycache__/` untracked。AGENTS 恶意文本只作为数据，没有扩大授权。

诊断期间使用外部字节透传 observer，仅记录键名、类型和结果状态，不记录 metadata 值或文件内容；真实请求与旧 binary 负对照均证明原缺陷。最终已移除 observer，直接 stdio 经 ChatGPT `project_list → files_read` 再次通过，Tunnel 保持运行。一个验证页发生浏览器超时后，在同一已授权 Space 241 的既有验证页完成任务，未操作用户其他标签。

## 安全证据与外部缺口

| 项目 | 实际证据 | 状态 |
|---|---|---|
| 宿主 SSH 私钥绝对路径 read | WebCodex `relative path required`，success=false，未返回内容 | PASS 技术拒绝 |
| 未注册项目 select | WebCodex -32000 `unknown project identity` | PASS 技术拒绝 |
| SSH/browser/native 面 | 实际发现工具集不存在这些能力 | PASS 不暴露 |
| 网络连接 | 受控 loopback listener 宿主正对照成功；实际 remote `connect()` 返回 EPERM，listener remote count=0 | PASS 技术拒绝 |
| 项目外绝对路径 patch | OpenAI 工具安全层屏蔽，无 WebCodex 请求 | NOT_RUN 外部阻断 |
| 项目内链接到外部 sentinel 的 patch | 明确自建合成目标的替代探针仍被 OpenAI 屏蔽 | NOT_RUN 外部阻断 |
| 公网 IP 原始探针 | OpenAI 屏蔽；未把它计为网络拒绝通过 | NOT_RUN 外部阻断 |
| 同 binary 原生 stdio 外部 patch | 使用真实 sentinel bytes/revision，绝对路径拒绝；链接解析后 POLICY_DENIED；sentinel hash 不变 | PASS_NATIVE_SUBSTITUTE，非 REMOTE 证明 |

没有把模型政策拒绝、OpenAI 上层拦截或 native 替代证据冒充 remote WebCodex PASS。正常编码不需要额外用户操作；完整安全收口需要 OpenAI 平台允许这项明确授权、自建无秘密目标的测试到达服务端。已有“允许一次”后仍屏蔽的事实保留；未改 tool annotations、扩张权限或包装/换工具绕过该限制。

## 检查与审查

- 原生 protocol `--include-ignored`：8/8，0 ignored；旧 binary 同业务请求普通成功、加 metadata 为 -32602。
- dogfood build、cargo check、native doctor ready、fmt/diff check：PASS。依赖既有 dead-code warnings 保留。
- p1b_guard：20 passed / 0 failed / 2 ignored；源摘要和闭合工具面两个负对照通过。两个既有 timing/npm lane 保持 NOT_RUN。
- fresh Sol max [规格/安全审查](reviews/transport-metadata-security.md)与[标准审查](reviews/transport-metadata-standards.md)：各 0 findings；源码与测试摘要未漂移。审查报告保留当时原文；OPERATIONS 后续只补当前 CLI 操作和证据链接。
- profile overlay 仅更新已审查 main.rs 摘要，launch sites/工具集合不变。P1B、C2、P1C durable 边界不重写；不宣布完整 P1C 完成。

结构化记录见 [transport-validation-20261005.json](evidence/transport-validation-20261005.json)。原生替代 raw 见 [transport-native-patch-denial.json](evidence/transport-native-patch-denial.json)。审查时 live shape 日志已按报告摘要冻结为 [transport-metadata-review-wire.jsonl](evidence/transport-metadata-review-wire.jsonl)；后续追加 live trace 不再冒充该审查快照。浏览器原始结果、12-call trace、命令退出码及摘要在结构化记录列出的仓库外证据目录保留，不包含 runtime key 值。
