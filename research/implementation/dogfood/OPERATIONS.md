# chatgpt-safe：本地编码入口

这是面向单个已注册项目的 macOS stdio MCP 入口。它复用已接受的 P1B ExecutionBroker 和 Git broker，不接入完整服务器的通用工具分发。

允许的工具只有 `project_list`、`project_select`、`files_search`、`files_read`、`files_apply_patch`、`shell_run`、`git_status`、`git_diff`。Jobs、LSP、SSH、provider、浏览器、native app、detached/session 和通用 gateway 不暴露。注册和权限升级均不是模型工具。

## 权限和结果

操作者在项目外准备一份 JSON registry：

```json
{"id":"dogfood-project","name":"Disposable coding project","root":"/absolute/path/to/narrow/project"}
```

根目录必须是已存在的窄绝对目录；`/`、HOME 及常见集合根被拒绝。进程启动时固定注册信息，每次数据操作重验根目录并要求该 `project_id`。`project_select` 只确认身份，不产生新的授权。要更改注册信息必须停止并重新启动服务。

文件工具只接受项目相对路径，真实目标仍须在项目内；`.git`、敏感名称及其链接别名被拒绝。普通项目内链接允许读取。读取每次最多 1400 字节，可通过 offset 继续；搜索最多检查 1000 个文件、返回 40 个匹配，并明确标记部分结果。权限或目录遍历错误不能成为完整的空搜索。Patch 只替换已有文件中唯一、精确匹配的旧文本，并检查 SHA-256 revision；不创建文件。遇到 stale revision、timeout 或 incomplete，应先重新读取当前内容，再决定是否继续，不能盲目重复写入。

文件 helper 使用操作者固定的 `/opt/homebrew/bin/python3 -I -S`，在 broker 子进程中解析、读取和修改。Shell 使用 `/bin/sh`，stdin 为 EOF，环境为 Minimal，PATH 固定，HOME/TMPDIR 绑定项目，网络拒绝。项目外执行权限不能由 cwd、环境、请求参数或仓库说明文件扩大。复用的 P1B 平台和工具链只读例外仍存在；这里不声称所有项目外字节一概不可读。

`shell_run` 最长 30 秒，每个输出流保留最多 12 KiB。成功仅表示直接子进程、所属进程组和捕获流完成；timeout 不是整个后裔家族的时限。Daemonize、setsid 和 durable 执行不受支持：离开进程组并关闭输出的后裔可能继续修改项目，不能依赖普通进程组完成状态来证明它已终止。后裔仍继承 kernel 文件/网络限制。超时、不完整和截断结果不计成功，可能已产生副作用；重新运行前检查项目状态。需要完整 durable supervision 时必须走后续 P1C 架构，不能靠命令字符串检查替代。

Git 工具使用固定只读参数，禁用外部 diff/textconv、fsmonitor 和 hooks；结果保留失败、timeout、capture incomplete 和 output cap 状态。`git_status` 的三次读取共用一次 15 秒操作预算。

## 构建和本地操作

```sh
cd /Users/songshiyao/Desktop/Projects/webcodex
cargo build --locked -p webcodex-chatgpt-safe --profile dogfood
BIN="$PWD/target/dogfood/webcodex-chatgpt-safe"
REGISTRY=/absolute/path/outside-project/operator-registry.json
"$BIN" doctor --registry "$REGISTRY"
"$BIN" status --registry "$REGISTRY"
"$BIN" serve --profile chatgpt-safe --registry "$REGISTRY"
```

Doctor 运行真实固定 Python sandbox probe。`ENV_BLOCKED` 和 `HOST_UNAVAILABLE` 是非通过状态；不能为了使其绿色而降级到裸执行。Status 的 `configured` 只说明配置可读，不证明服务存活。Serve 为前台 stdio：stdout 只承载 JSON-RPC，stderr 承载启动错误；正常停止可关闭 stdin，交互运行可 Ctrl-C。协议 `ping` 检查连接存活，`tools/list` 核对八个工具；工具列表可读并不证明 broker 可执行。

本次构建使用外部 Cargo target 目录，避免恢复整个仓库的大型缓存。交付时的二进制路径和摘要由 `EVIDENCE.md` 记录。把含真实密钥的项目注册给模型前，应按该项目的实际信任边界选择内容；本次验证只注册 disposable fixture。

## Secure MCP Tunnel

使用官方 tunnel-client 的 stdio transport，不启动无认证 public listener。当前隔离安装为 `0.0.15`，未替换仓库原有 tunnel pin，也没有修改全局配置。Tunnel ID 和 runtime key 尚未配置，因此真实 ChatGPT 读写能力为 **NOT_RUN**。

先由操作者从 OpenAI 组织取得真实 Tunnel ID，把 runtime key 保存到项目外的权限受限文件，命令只使用 `file:` 引用。不要把 key 放进 registry、MCP 参数、命令字符串或对话。

```sh
TUNNEL_CLIENT=/absolute/path/to/tunnel-client
PROFILE_DIR=/absolute/private/path/tunnel-profiles
TUNNEL_ID='operator-supplied-real-tunnel-id'
KEY_REF='file:/absolute/private/path/runtime-key'
BIN=/absolute/path/to/webcodex-chatgpt-safe
REGISTRY=/absolute/path/outside-project/operator-registry.json

"$TUNNEL_CLIENT" runtimes connect --alias chatgpt-safe \
  --profile chatgpt-safe --profile-dir "$PROFILE_DIR" \
  --tunnel-id "$TUNNEL_ID" --runtime-api-key "$KEY_REF" \
  --mcp-command "$BIN serve --profile chatgpt-safe --registry $REGISTRY"
"$TUNNEL_CLIENT" doctor --profile chatgpt-safe --profile-dir "$PROFILE_DIR" --json --explain
"$TUNNEL_CLIENT" runtimes status chatgpt-safe --json
```

以上变量是操作者提供的占位说明，不是已配置的现场资源。路径含空格时，应根据官方 CLI 的 stdio command 解析规则引用，不能由模型传入任意 MCP server command。只有实际 status 明确给出进程运行、health/ready 成功才报告服务已启动。停止使用 `tunnel-client runtimes stop chatgpt-safe`；不要使用 nohup/disown。受管 status 返回的 loopback health URL 才是实际监听地址，不能假定默认端口可用。日志留在官方 runtime/profile 的 operator 日志路径；MCP stdout 不写日志，不开启 raw credential/payload logging。

操作者在 ChatGPT developer-mode 添加该 Tunnel、refresh tools，并核对恰好八个工具，然后实测 read 和需要确认的 write。组织 Tunnel 权限与 ChatGPT 账户功能是不同的门禁，不能从套餐名称或本地测试推断权限。当前 RDC 仍提供任意宿主命令和文件能力，未提供可证明的项目 ACL，因此没有被接受为安全后备 transport。

官方依据：[Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)、[Connect and test](https://developers.openai.com/plugins/deploy/connect-chatgpt)。
