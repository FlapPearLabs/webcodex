# C2 guard overlay：专用孤儿链退休补充

2026-10-04，架构负责人确认；待独立 Sol 以最终 scope/source 验收。沿用 V1 的 immutable P1B、固定白名单、逐项审核与 fail-closed 规则。V1 是当时尚未退休专用 transport 的草案，不删除或冒充最终源绑定。

## 固定变动集合

起点 `f58e65c6d95bbd91165e97b4a98de694f97ae872`。accepted P1B JSON SHA-256 固定 `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee`，原字节与全部 logical classification/accounting 不变。

Production origin replacements **必须恰为以下五项完整集合**，每项匹配旧 SHA、targets、path 后替换成审查后的新 SHA，不允许运行时自动采纳 source 内容：

- `crates/webcodex-runner/src/main.rs`
- `crates/webcodex-runner/src/webcodex_runner/mod.rs`
- `crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs`
- `crates/webcodex-runner/src/webcodex_runner/ssh.rs`
- `crates/webcodex-runner/src/webcodex_runner/transport.rs`

Production origin removal **必须只有一项** `crates/webcodex-runner/src/webcodex_runner/remote_shell.rs`，匹配该原 origin 的 SHA/targets/path。这是唯一、已签署退休的 C2 专用模块；不得为新增通用 removal/目录排除提供入口。

Raw/reference removals 仅为：退休模块内原 rows、`PersistentShellManager::exec_ssh` 旧写入/观察与由此退休的专用 helper rows，以及 SSH 专有 preparer/availability 的原 rows。最终具体完整键（file/symbol/primitive-or-reference/count/targets）由实际扫描与源码核对后列入 overlay，必须唯一匹配原 inventory。任何行若牵涉共享/B22 source、其他 C surface 或不明新行，先拒绝，不自动接纳。不得新增 launch rows。

## 应用与证据

Versioned `c2-guard-overlay.json` 仅有固定 schema/status/base SHA/base inventory hash、精确上述 origins 集合、审核后的 raw/reference removals。`accepted_inventory` 在内存应用，保留全部其余 P1B targets、origin 源、body、boundary refs、non-Rust assets、logical_surfaces/counts，不得由 overlay 改变 classification/accounting 或未审核源。

拒绝未签 status、base hash/source old-hash 不符、重复 path/row、缺失固定 origin 项、不存在 removal、未知 payload 字段以及任何扩大集合。继续使用既有 real-production module AST scan 和 exact byte hashes；不要重写 scanner，也不要给当前工作树设置自动更新开关。

必要验证：正向完整 guard；无效 overlay 正向拒绝；一次在 hash-guarded 自有 C2 源或同源 scratch 中插入真实 production launcher/复活退休 remote 创建原语，同一 production gate 必须 Cargo 非零 RED；恢复 exact source 后相关 gate 回归。保存 raw 命令/exit/source/mutation/restore/hash。只包含 assert-error 的绿色 unit harness，不单独作为实际 mutation RED。

C2 milestone只关闭 `shell.persistent-remote.unix/windows`，记录 `DEFERRED_FAIL_CLOSED` 与 remote authority backend next contract。其余 C7 仍为 `ARCHITECTURE_DECISION_PENDING`，不能自动计 migrated、deferred-fail-closed 或 P1C PASS。当前物理生产创建面减少必须解释为 C2入口/专用模块退休，保留原 accepted 9 logical IDs 的历史/处置账本。
