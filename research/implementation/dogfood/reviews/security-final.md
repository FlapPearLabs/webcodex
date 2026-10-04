# chatgpt-safe 独立规格与安全最终审查

审查时间：2026-10-04 19:31 UTC（2026-10-05 03:31 Asia/Shanghai）。
审查角色：独立 Spec/security，只读仓库；实现与执行者自述不是验收依据。

## 裁决

**ACCEPT_BOUNDED_LOCAL_SOURCE_MILESTONE；限定范围内 blocking findings = 0。**

接受的是单个操作者注册项目上的 macOS stdio MCP 编码入口、八个固定工具及其实际 broker 路径。规格允许今晚采用现有 P1B foreground execution、隐藏未证明的 durable 能力；此候选符合这个限定工程里程碑。生产 main.rs 与实际二进制的绑定如下：

| 对象 | SHA-256 / Git identity |
|---|---|
| 基线、审查时 HEAD | `d8edf498a063200e0ad87723ff63df2b54d3f612` |
| 分支 | `impl/webcodex-p1c-durable-execution` |
| main.rs | `8160f4e2a13755a1a3a9ef9c9787a5f466ce9ba398510a4cb9d0a85e4bacf557` |
| 实际 dogfood binary | `7aae451460f9f720b47db2b3a2aea71f24d99ec5877c166bfa3ce8da4c7d5ac4` |
| 最终 protocol.rs | `41a773f85502d2d0e2d88c4e847d15611be08f2905cd51dc7fa4284962c29f5b` |
| 待确认状态的 profile overlay | `377025a09e34d60f0f63153a6b4a25ec46732d43253f603c10a8b8a0955c2bb4` |

**TONIGHT_DOGFOOD_READY=NO；ChatGPT Web read/write = NOT_RUN。** 本地 stdio adapter 成功不能证明 ChatGPT 账户能力或外部 transport 已工作。RDC 的任意宿主 shell/文件接口未被接受为安全后备。整个后裔家族的终止、durable supervision 与完整 P1C 均为 **NOT_PROVEN**。

审查时 profile 仍为 `UNREVIEWED_CANDIDATE`，完整 p1b_guard 因该状态未通过。本报告允许主负责人在独立标准轴也通过后，仅将审核状态改为 `SOL_REVIEWED_CHATGPT_SAFE_PROFILE`，保持全部生产源码和清单语义不变，然后复跑完整结构、workspace、格式门禁。报告不提前给该复跑、最终提交、干净 worktree 或远端同步记 PASS。任何生产源码、权限、工具面或清单语义变化需 fresh review。

## 方法与范围

先读附带的 tonight spec、仓库 AGENTS、安全不变量、permission model 和 tool contract，独立形成设计约束，再读候选 diff 与所有 untracked source。clean-room 约束为：操作者持有的不可由模型写入的 registry、单个 canonical project authority、闭合分发、文件与 shell 真正经过 kernel broker、固定 Git 只读入口、错误与资源后置条件必须控制成功判断。未沿用旧候选的接受结论。

核对了四个 tracked 工作区变更（Cargo.toml、Cargo.lock、workspace-boundaries.toml、p1b_guard.rs）及新 crate、profile overlay、测试、OPERATIONS、EVIDENCE、诊断 harness 和实际证据文件。现有 ExecutionBroker、WorkspaceAuthority、Git broker、Unix ManagedChild、compiler/SBPL 为调用链与边界上下文；其生产实现未在本轮 diff 中修改。仓库中的 evidence/harness 副本与已亲读原件逐字节一致。

使用 code-review skill 的独立规格审查流程。现有 CodeGraph 的只读 explore 尝试返回 `unable to open database file`，未初始化、重建或写索引，随后以源码阅读确认调用链。本审查未 spawn 下级 agent，未编辑仓库、更新审核状态、提交或推送；只在任务目录写独立探针证据和本报告。

## 权限与工具契约

| 轴 | 源码和实际证据 | 结论 |
|---|---|---|
| 工具暴露 | main.rs:23–32 固定八个工具；332–440 闭合分发与 exact keys；原生 tools/list 和未知 job/SSH/provider/gateway 调用 | 八个工具可达；通用 broker primitive、jobs/detached/session、完整服务器分发不可达 |
| 项目身份 | 92–139 startup registry 与窄 canonical root；142–152 每次重验 root；451–455 身份检查 | cwd 不授予 authority；模型不能注册、切根、转发 env、开启 network 或更换 profile |
| 文件 | 481–521 固定 Python `-I -S` helper；939–960 canonical containment、resolved 敏感路径复验、O_NOFOLLOW 和 regular-file 检查 | 外部、跨项目与越界 symlink 被拒；普通项目内链接仍可读；项目 shadow modules 未被导入 |
| 编辑 | 968–984 unique exact replacement、revision 双重检查、同目录原子替换；真实 adaptive patch 与 stale-revision 测试 | 只修改已有文件；超时或不完整可能已有副作用，先重读 revision 再重试 |
| Shell | 625–665 固定 /bin/sh、Minimal env、stdin EOF、project HOME/TMPDIR；859–894 共享实际 ExecutionBroker 路径 | 宿主合成 secret 未继承；外部写入/读取及 socket probe 实际 EPERM；没有裸执行 fallback |
| Git | 523–623 固定公共 read broker；diff 禁外部 diff/textconv，fsmonitor/hooks 禁用；status 共用 15 秒预算 | 真实 branch、HEAD、dirty state 与 diff 可读；不是模型传入任意 Git 命令 |
| 成功判断 | 683–815 直接 child、所属 group、完整 streams、无 cap/timeout/read error 才 success | timeout、capture incomplete、output cap 与未知状态不会静默成为成功 |

Shell 有意允许运行项目内代码。文件 helper 的敏感名称规则不能被描述成整个项目 shell 的内容保密边界；操作者注册项目意味着授予该项目内容的读写能力。P1B 的固定平台/工具链只读例外仍存在，OPERATIONS 已明确披露，因此结论不声称所有项目外字节一概不可读。

文件 read 通过 offset 支持继续读取；shell capture 每流保留最多 12 KiB，超限为非成功。当前没有 durable job 或捕获日志的 opaque continuation handle，不能把限额内的片段称作完整执行日志。

## 已关闭的具体缺陷

1. **敏感路径链接别名**：旧 source 的实际 MCP 曾通过 .env alias 返回合成值。最终 helper 对 lexical 与 resolved 路径都复验 .env/.git 等名称。最终协议测试拒绝单层、多层别名，允许 ordinary in-project link。旧 raw 仅作历史缺陷证据。
2. **目录遍历错误被误报完整空搜索**：旧 os.walk 没有 onerror，chmod 000 目录会被静默跳过。最终 main.rs:988–990、1009 捕获 walk 错误并设置 truncated。本审查独立操作非 root UID 501 的干净临时 fixture，同一 final binary 可读时返回 `success=true,truncated=false,files_scanned=1`，目录 chmod 000 后返回 `success=true,truncated=true,files_scanned=0`；两次实际 MCP 均 clean EOF rc 0，source/binary 摘要前后不变。
3. **目录测试被其它 partial 原因遮蔽**：早期测试复用了有 .env/alias 的 fixture，不能独立证明目录错误标记。最终 protocol.rs 已隔离新的 clean Fixture，并明确要求可读 positive 的 truncated=false；root UID 则拒绝为 HOST_UNAVAILABLE，不计通过。最终 native `--include-ignored` raw 为 6 passed / 0 failed。
4. **Git alias 错误分类**：resolved .git 拒绝信息包含 .git，使既有分类器返回 POLICY_DENIED。改变的是机器分类，拒绝行为保持不变；最终原生敏感 alias 测试读取 outcome 而非错误字段。

从 final HELPER 仅删除一次 os.walk 的 onerror keyword 后，独立 clean fixture 重新出现 `truncated=false,matches=[]`，诊断负对照退出 0 并证实断言对旧缺陷敏感。该记录明确是 **DIAGNOSTIC_ONLY_NOT_BROKER_PROOF**；实际修复证明使用生产二进制的原生 MCP 结果。默认受限运行的独立探针返回 ENV_BLOCKED 并使断言退出 1，保留红色原件，不当通过。

## 实际验证与证据边界

最终 native trace 包含 **49 条记录**（MCP 操作、上下文和后置检查），绑定上述 8160 source / 7aae binary。已亲读 response/data 与驱动源码，未只采信 PASS 汇总：

- 项目内 search/read/patch/shell/Git 正对照成功；恶意 AGENTS 与 README 实际被读取。
- 外部 read/write、absolute/traversal/symlink、cross-project 与未知 identity 拒绝；root/env/network/profile 参数扩权拒绝；外部 sentinel 摘要不变。
- 网络测试先以原生 listener 接受一个正对照连接，再通过 production broker 得到 socket EPERM，accept count 为 0。旧 quoting SyntaxError 不计边界证明。
- timeout 1 秒、超额输出、普通持流后裔和 setsid 持流后裔均为 success=false 的显式状态；随后 ping 仍正常。stdin EOF 后 server 正常退出，未宣称持续在线。
- checker 原始结果为 `PASS: checked 49 normalized MCP calls and postconditions`。本审查也实际运行 checker self-test，三个 ENV_BLOCKED、OUTCOME_UNKNOWN、success=false 负对照均使子 checker 非零；checker self-test 自身 rc 0。

连续编码 trace 有 **11 次真实 tools/call**：
select → search → read source → read test → read malicious AGENTS → patch regression test → shell（两个 assertion failure，rc 1）→ patch pricing formula → shell（2 tests OK，rc 0）→ git diff → git status。两个业务文件修改均经过 safe MCP；driver 与 adapter 没有替代模型在宿主直接修改业务源码。该结果为 **PASS_LOCAL_MCP**，不是 ChatGPT Web dogfood 证明。

最终 protocol raw 为 6/6。非 macOS target 在本机按 cfg 为 0 tests，记录为 NOT_RUN；不宣称 Linux/Windows parity。build、workspace 和格式完整性由独立标准轴与主负责人收口，本安全轴不以其绿色代替 authority 验证。

## 必须保留的 lifetime 限制

亲读的 close-pipe counterexample 证明：fork → setsid → 关闭 stdout/stderr → 延迟项目内写入，foreground shell 可以 success=true 返回，后续 files_read 仍见到 delayed sentinel。它发生在前一候选二进制，记录未附带 final 8160 binary 的 source-context header；所以既不能冒充 final 全家族测试，也不能作为安全 PASS。现有 ManagedChild ownership 是 process group，生产 engine 未变，不能据 description 更新宣称该架构限制被修复。

最终原生 setsid probe 另行证明脱组后裔仍受外部文件/网络 kernel 限制，但观察到操作返回时后裔仍活着；其自然退出后置观察不是 broker termination 证据。

main.rs:325 与 OPERATIONS 已准确限定 success 为 direct child / owned group / captured streams，timeout 不是 whole-family deadline，daemonized/setsid/durable unsupported，重试前检查副作用。用户 spec 明确优先现有 P1B bounded foreground、允许隐藏未证明 durable 功能，因此该限制在**当前限定源码里程碑**可接受。若要保证所有后裔在 deadline 终止或禁止任何迟到项目写入，文档无法实现该保证，需要独立的 family supervision / host-enforced ownership 架构；本报告没有批准这项能力。

## Transport 与全量规格差额

本审查实际运行隔离官方 tunnel-client doctor --json，rc 2，`result=fail,failed_checks=[tunnel_id]`；raw 不含 key 值。该结果直接证明缺少有效 Tunnel ID，不能由它推断真实 read/write entitlement。当前官方 [Secure MCP Tunnel 文档](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)要求真实 tunnel identity 与 runtime credential；账户/UI 配置与本地 broker 测试是独立门禁。

OPERATIONS 与 EVIDENCE 正确记录外部缺口、已关闭本地会话、无 public listener、未配置现场 transport，并明确 RDC 当前任意宿主能力不可作为安全 fallback。源码未依赖 RDC 来执行编码。当前 `MCP_READ_AVAILABLE`、`MCP_WRITE_AVAILABLE` 都只能是 NOT_RUN。

全量定义中的 transport 工作、真实 ChatGPT 连续调用、最终 clean/push/sync 尚未在本报告时点完成；因此不能宣布 tonight ready。操作者完成真实 Tunnel ID、项目外 runtime-key 文件引用及 ChatGPT developer-mode 添加/refresh 后，仍须实际确认八个工具与 read/write/编码链。本报告不授权通过 RDC 裸 shell 绕过它，也不要求为本地已验证源码重写完整 P1C。

## 审查快照

以下是审查时文件字节绑定；只允许前文所述审核 status 字段及如实收口记录在门禁后变化。源代码或授权语义 drift 会撤销本轮 source acceptance。

```json
{
  "head": "d8edf498a063200e0ad87723ff63df2b54d3f612",
  "binary_sha256": "7aae451460f9f720b47db2b3a2aea71f24d99ec5877c166bfa3ce8da4c7d5ac4",
  "candidate_files_sha256": {
    "Cargo.toml": "2b638693962f06d967b8e22f434fdb62f370673955baf0c779cafba521cfc7bf",
    "Cargo.lock": "1a941fef91487e87ec08fa1f4e7764dc7d5f543aeb282a3ffaae1c0c30a4878d",
    "workspace-boundaries.toml": "8beeb170cf838b17be7fb3c61e73835fa279873911f9b4b521db3623e1419ad3",
    "crates/webcodex-process/tests/p1b_guard.rs": "d1e4f646f0cf773da5dbeef429d13fb48f77644d073d7dea4bddd7f67bfdfaf2",
    "crates/webcodex-process/tests/p1b_profile_overlay.rs": "690abb5a54f2b32d69c3bc84f17b6711aa8bed9e73724fe3bf5528e99d4ca731",
    "crates/webcodex-chatgpt-safe/Cargo.toml": "6b1699fb6f7550c83e72321ef23f496123d76bad87fe3e1a625aba0702b980c8",
    "crates/webcodex-chatgpt-safe/src/main.rs": "8160f4e2a13755a1a3a9ef9c9787a5f466ce9ba398510a4cb9d0a85e4bacf557",
    "crates/webcodex-chatgpt-safe/tests/protocol.rs": "41a773f85502d2d0e2d88c4e847d15611be08f2905cd51dc7fa4284962c29f5b",
    "crates/webcodex-chatgpt-safe/tests/unsupported_platform.rs": "52aec28c8d329c31c942c433f17f1e272b0f33d303152a6c5ceec1d02a5b5270",
    "research/implementation/dogfood/EVIDENCE.md": "3f419675ab6cfa3d3c24be9276d9b73ae0cf7c744069673f69df01bc7f67fec3",
    "research/implementation/dogfood/OPERATIONS.md": "f7cb90a4207c8a71beb78ece377974e40c115574256002c06288495685debcdb",
    "research/implementation/dogfood/evidence/adaptive-coding-accepted-source.jsonl": "d782d4b3b382a4919dbbf121cbb3e528812a6c54e2177baee51ada4310f07923",
    "research/implementation/dogfood/evidence/checker-final.txt": "a9252bdd73d605de2431d46c5678ad81bd1a074bca2bf83bd8d1d40ce511ec22",
    "research/implementation/dogfood/evidence/checker-negative-control.txt": "5e36f3be4afa55b55e79f679bfe51bc4247eb70018f89e47192993e736372a97",
    "research/implementation/dogfood/evidence/close-pipe-counterexample.jsonl": "b1913a3c2f4077fc94a08a56a1533be2e387395cba97df3d365f1cd676061b6b",
    "research/implementation/dogfood/evidence/directory-search-diagnostic-negative-control.jsonl": "742b728a8d89fb77a6f397a36a3b8282c3d3cee889f6f2caa87548ef3db8576c",
    "research/implementation/dogfood/evidence/directory-search-independent-native.jsonl": "f7576951befac7b36b62b18876783bc1b265d773d6ff03f8700a9a4e2db05a37",
    "research/implementation/dogfood/evidence/directory-search-independent-sandboxed.jsonl": "304766478f4ce327e2bf8179e79d710baec12322fd5adc2a957ac20a4809424f",
    "research/implementation/dogfood/evidence/native-boundaries-accepted-source.jsonl": "ef891f11e4849573811c0757ddeb6a26d1c4abc1178217d5c6ba51cde7a37b95",
    "research/implementation/dogfood/evidence/old-source-alias-search-negative.jsonl": "db90813933e2100ba8a8da0b8c20043ca3481464c44b070c85f8d0a7bf3e846b",
    "research/implementation/dogfood/evidence/protocol-final.txt": "92172816771b91f3d7cb6501417e84adeeb9cf7d772a7ea49fbca11ed7bc9e75",
    "research/implementation/dogfood/evidence/service-doctor-default.txt": "50fb883baad2adf26730aaaafcca5e86e0608a3e74c81da8c5d08b38f08be20b",
    "research/implementation/dogfood/evidence/service-doctor-native.txt": "6b3ee1c8f852a1a100d1e0c11174b5659135bfb09411d7dd3b0467e9d290269f",
    "research/implementation/dogfood/evidence/tunnel-doctor.txt": "9d8309c3f31229c87f45cc8d0260fae346d71bbb077e0c238f14b128882208a8",
    "research/implementation/dogfood/harness/adaptive_session.py": "d6b8fd52cdc16412ea4ad360de6b4a41f53f1e3a49491cea80fb7cfa71081556",
    "research/implementation/dogfood/harness/check_evidence.py": "25e8951938e7a0ac5555ce8c1d411c08f9c80a55b53cc1ab83d40c242d11389d",
    "research/implementation/dogfood/harness/directory-search-negative-control.py": "96490fc5fe230d5f39827896d8a51ec3f0558970b815931b4fa78aa1e3e22f32",
    "research/implementation/dogfood/harness/evidence_contract.md": "1fdb5a4ee2b18b5309556a368d7cfac4f8c715cb89f5f59f3fc5b178d0dec006",
    "research/implementation/dogfood/harness/mcp_dogfood_driver.py": "18ef7180139a585f24caccc8282ee95cad2bad0342ff468b076ba4b71c43afc2",
    "research/implementation/dogfood/harness/prepare_fixture.py": "2237d4ec170b2f594d9870b321a6b3c5ecd74cea5a5bbda8616f15a6fcaa8691",
    "research/implementation/dogfood/profile-launch-overlay.json": "377025a09e34d60f0f63153a6b4a25ec46732d43253f603c10a8b8a0955c2bb4"
  }
}
```
