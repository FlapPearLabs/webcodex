# C2 native evidence：固定生产 Runner 与无副作用拒绝

只验证 C2 被关闭的入口，不作为 remote-host confinement、detached lifetime 或整个 P1C PASS。此前 focused Runner test binary 用 bootstrap Feature 仅为 DIAGNOSTIC，不能代替本证据。

生产 Runner 使用未加 bootstrap/feature/shim 的普通 dev build，指定 repo 当前冻结 source 与 Cargo.lock，离线 locked 构建。记录 source hashes、实际 compiler/cargo/platform、构建命令/exit、binary SHA。不得使用旧缓存 binary 自称当前源。

Fixture 可复用 P1B catalog polling-harness 的真实 HTTP register/poll/result 协议，或既有 WebSocket 协议。仅 loopback synthetic 服务、temp accepted project/registry/state/config/token，真实 Runner 自有进程，以明确 stdin/stdout/stderr、总 timeout 与 finally exact owned child/group cleanup；不得接入实际 SSH host/credential。

Runner 子进程的 HOME/PATH 可用 Command/Popen 的独立 env 给定 synthetic fixture 值，不改全局环境。fixture 提供假 `ssh` 可执行与专属空配置：固定已有控制面 `ssh -V` 探针允许并单独记录；所有模型资源相关 `-G`/prepare/connection/bootstrap 调用留下 marker 后立即失败，不访问网络。不能把注册时的既有固定能力探针混为 remote request 执行。

验证：实际 registration 中 ssh_persistent_shell 为 false，local persistent_shell/one-shot ssh_shell 与既有 builder 相符；向真正 poll/dispatch 发送合法 named-resource remote open 与 exec，收到稳定 remote_durable_authority_unavailable、command_started=false、没有模型 SSH prepare/connect/spawn marker。temp resource/project 必须有效，不能拿非法边界早退代替新拒绝。本地 fallback 若列作 native 实测，另需临时本地 payload/profile-init marker 的有效正控制与拒绝阶段 marker=0；仅 SSH marker=0 不能证明 local 无副作用。没有该正负 fixture 时，local fallback 项明确为 SOURCE_DERIVED_ONLY，不能提升为 native PASS。

反证：同 fixture / 同行为断言在 exact accepted-base 生产代码的 isolated scratch copy 执行。它恢复真正原 facade、SSH pool/preparer、transport module、capability，而非只取消顶层 guard、或仍用拒绝 helper。先实际 collect marker/result，必须观察模型 prepare/connect/bootstrap 至少一项 marker>0，而且同一 no-effect 行为断言必须导致实际非零；两者缺一不可。记录源码/hash/命令/raw/exit。必须排除 compile failure、缺 fixture、超时或 cap/error-code 检查先失败掩盖 marker；只有 capability/error-code 差异不能替代本负控。negative-control 只证明旧路径有模型准备/启动效果，不让假 SSH 成功伪装真实 remote sandbox PASS。

omitted-resource 存量 remote identity 的写入 fence 由同源 diagnostic SpyTransport 的 write_count 及其已跑 mutation RED 支持；生产新 Runner 无法新建远端 entry，不能通过测试专用状态注入自称 native legacy-transport recovery proof。其余 restart/FD/outcome C7 实施仍等待真正架构裁定。
