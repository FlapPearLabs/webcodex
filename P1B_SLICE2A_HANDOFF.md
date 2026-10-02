# WEBCODEX P1B SLICE 2A — 接手 Handoff（含工作区冲突告警）

> 文档定位（接手 agent 必读）：本文件是真实交接物，不是聊天文本。
> - **本机绝对路径**：`/tmp/wcb_handoff/P1B_SLICE2A_HANDOFF.md`
>   （该路径属于一个隔离 git worktree，不是主工作区；主工作区见下文「⚠️ 工作区冲突」）
> - **远端分支**：`origin/handoff/p1b-slice2a-review`
>   取回方式：`git fetch origin handoff/p1b-slice2a-review && git checkout FETCH_HEAD`
> - **本文档 commit**：`83a58939b6bc25a31920f54dcf5d5adc0852bc9d`（本文件所在 commit）
>   已由 `git ls-remote origin refs/heads/handoff/p1b-slice2a-review` 实证返回一致。
> - **本文档评审基线**：`0f56dd81aed0e63100d537a1dbc4e820e0356424`
>   取回后请 `git checkout 0f56dd81aed0e63100d537a1dbc4e820e0356424` 读真实代码；
>   本文档基于该 SHA 的**静态审阅**（未运行测试）。
> - 原始对话产出曾**只存在于聊天窗口** → 下一个 agent 看不到，已被判为交付失败。本文件是为纠正该失误而写。

---

## 0. 状态总览（2026-10-02 20:11 现场核验）

| 项 | 值 |
|---|---|
| 仓库 | `/Users/songshiyao/Desktop/Projects/webcodex` |
| 主分支 | `impl/webcodex-p1b-normalization-slice1` |
| 最近生产 commit | `0f56dd81aed0e63100d537a1dbc4e820e0356424` |
| 主工作区状态 | **非 clean**：`git_broker.rs` 有 380 行未提交改动（**他人写入，非本 handoff 作者**） |
| 本 handoff 所在 | 隔离 worktree `/tmp/wcb_handoff`，基于 `0f56dd81`，CLEAN |
| VERDICT | **NOT_A_PASS**：Slice 2A 未被 Codex 接受；且有未提交的他人修复待裁决 |

---

## 1. ⚠️ 工作区冲突告警（接手前必须先读）

**主工作区 `git_broker.rs` 有 380 行未提交改动（`+380 / -117`），最后修改时间 19:24:41，HEAD 仍是 `0f56dd81`。**

这不是本 handoff 任何一轮产生的，是另一个 agent（或用户）在 2026-10-02 18:55~19:24 之间写入的
**ISSUE A / ISSUE C 修复**。本 handoff 作者**未触碰、未 commit、未 stash、未 reset** 这批改动。

该改动已包含的内容（grep 确认）：
- `clock_now!` 宏 + `TestClockGuard` / `TestClockSnapshot`（枚举 `ClockPoint::{Operation, TailOpen}`）
- 新测试 `bounded_capture_opens_its_tail_after_the_direct_child_wait`
- 新测试 `bounded_read_opens_its_tail_after_the_direct_child_wait`
- 新测试 `probe_deadline_applies_one_fixed_internal_cap_without_extending_caller`
- `open_drain_tail` 调用点从函数入口**移到了 child wait loop 之后**（cleanup entry）
- `probe_deadline` 文档改为 "bounded by both the caller and the internal probe ceiling"

**这意味着：手写版 handoff（含行号 689/753/528-539/515）已全部失效**。接手时务必
`git status` + `git diff --stat` 重新核验，不要信任任何早于 19:24 的行号。

**对该 380 行改动的初步评估**（仅静态审阅，未经测试运行）：
- ISSUE A 看起来已被实质修复：tail 在 cleanup entry 开启，且用 test-clock 断言 `Operation < TailOpen`
  事件顺序 + tail 只开一次 —— **恰好规避了此前踩过的「时序 fixture 在两种实现下收敛成假绿」坑（P2）**。
- ISSUE C 已被裁定为 **OPT-2（保留 `min(caller, cap)`，修文档而非改语义）**：新增测试精确断言
  `tight(3s)→Some(tight)`、`generous(60s)→Some(t0+5s)`。契约争议在代码层面已闭合，但
  **这是另一个 agent 的裁定，仍需用户/评审认可**（见 §4）。

该改动 `cargo fmt --all --check` clean、`git diff --check` clean，但**未编译未测试验证**（本机无法跑 runner test）。

---

## 2. 六 SHA 时间线（真实 git log，author FlapPearLabs）

```
0f56dd81  2026-10-02 18:25  fix: one shared drain tail, propagated deadlines, honest blocked verdicts
accc4a87  2026-10-02 17:12  fix: close remaining git deadline leaks
94c209a7  2026-10-02 16:37  fix: bound the whole git operation, prove capability, let verdict fail the test
d23ef744  2026-10-02 14:59  fix: bound the reader drain, pick a functional git, and stop scoring blocked
1b4159e2  2026-10-02 13:45  refactor: route the project catalog through the git broker
0701cce3  2026-10-02 11:46  impl: route project overview through execution broker
```
线性链，无 merge，无改写。

### 各 SHA 改动文件
- `0701cce3`: `git_broker.rs`, `project_overview.rs`, `normalization_p1_tests.rs`, +2 docs
- `1b4159e2`: `catalog.rs`, `projects.rs`, `lifecycle.rs`, `managed_worktree.rs`, `unconfined_git.rs`, `git_broker.rs`, `lib.rs`, +1 doc
- `d23ef744`: `catalog.rs`, `git_broker.rs`
- `94c209a7`: `catalog.rs`, `git_broker.rs`
- `accc4a87`: `git_broker.rs` ← catalog F3 未触碰
- `0f56dd81`: `git_broker.rs`, `project_context.rs`, `project_overview.rs` ← catalog 未触碰
- **`CATALOG_F3_UNCHANGED = YES`**：`catalog.rs` 自 `94c209a7` 后未动。

---

## 3. `0f56dd81` 本身做了什么（已被 handoff 作者核验）

1. **单尾 drain**：`open_drain_tail(deadline) = min(deadline, now + 250ms)` 只取一次 min，得 `Instant`，
   `finish_bounded` / `finish_bounded_read` 内 tree/stdout/stderr 共享。彻底修复了旧版每阶段各 250ms = 750ms 的累积缺陷。
2. **verdict gate**：`git_broker.rs` 内 `enforce_verdict`（对齐 `catalog.rs:833`），Pass 放行，其余三态 panic。
3. **`project_context.rs`**：`BROKERED_GIT_USABLE: OnceLock<Result<(),String>>` 缓存一次 `git --version` 探测；
   `require_brokered_git!` 由 print+return 改为 **ENV_BLOCKED panic**。
4. **`project_overview.rs`**：保留 degraded-filesystem 断言（独立有效 claim），随后显式 panic（tracked-index 语义 UNMEASURED）。
5. **`finish`（无 caller deadline 路径）刻意不共享尾** —— 见 §5 坑 P1。

---

## 4. 待裁决 / 待闭环项（接手 agent 必须处理）

### ISSUE A（drain tail 起算时机）—— 在 380 行改动中已修复，待评审认可
- 旧问题：`open_drain_tail` 在 `finish_*` 函数入口调用，child wait loop 尚未结束就起算并可能耗尽 250ms。
- 380 行改动：调用点移到 wait loop 之后（cleanup entry）。
- **接手动作**：跑通该改动的编译与测试（见 §6 本机限制），确认 test-clock 断言通过，再把它 commit 化。

### ISSUE C（`probe_deadline` caller 绝对 deadline 传播）—— 另一 agent 已裁定 OPT-2，需用户认可
- 实测实现：`min(caller, now + 5s)`，非 exact propagation。
- 此前文档（L515 旧位置）称 "comes back as that instant"，与实现矛盾 —— **矛盾已在 380 行改动中消除**（doc 改述）。
- 现有测试只断言 `<= caller`，无法区分 exact vs cap。**380 行改动新增测试已精确锁定 OPT-2 语义**。
- **这是契约裁定，不是机械修复**；AI 不应单方面宣布。需用户确认「保留 min 封顶」是终态语义。

### 诚实性红线（不得违反）
- 本机 `sandbox-exec` 全局 `sandbox_apply: Operation not permitted` → 所有真实 brokered-git 测试本机只能报
  `ENV_BLOCKED`；**不得**宣称 native path PASS。
- `cargo test -p webcodex-workspace --lib` 期望 **88 passed / 12 failed**，12 个全是
  `ENV_BLOCKED ... UNMEASURED — this is not a pass`（设计使然）。**禁止**重新 skip 变绿。

---

## 5. 三个坑（接手 agent 不要重蹈）

- **P1**：`finish()`（无 caller deadline 路径）**不能**共享单尾。共享会让 stdout 用满 250ms、stderr 剩 ~0、
  `recv_timeout(0)` 返回**空 stderr**但调用报 success；`brokered_git_usable()` 靠 stderr 识别 sandbox refusal，
  空 stderr 会误判为可用，导致 8 个 `project_context` 测试真的去跑并失败。该路径无 caller clock，**每流各拿一份完整 budget** 才对。
- **P2**：时序类回归 fixture 会在「buggy 实现」和「修复实现」下**收敛成假绿**（两种实现一旦消耗了时间就一致）。
  两个失败尝试踩过此坑。正解：用 test-clock 断言**事件顺序**，或在测试内把旧表达式**作为值复算**比较计数（750ms vs 250ms），绝不 wall-clock 竞速。
- **P3**：本机 shell `grep -E 'a|b'` alternation 失效；裸 `grep` 偶发对存在模式返回空；`ps` 返回 `operation not permitted`。
  关键断言用 Read/sed 或 Grep 工具，不要信 shell grep。

---

## 6. 本机环境限制（真实，已实测更正）

- `cargo check -p webcodex-runner`（**crate 本体**）→ **编译通过**（2 warning）。
- `cargo check -p webcodex-runner --tests` → `error[E0658]: use of unstable library feature 'atomic_try_update'`
  @ `coding_agent.rs:261`。**后果**：runner **测试体**无法在本机编译 → catalog F3 的 `enforce_verdict` 测试
  **无法在本机复验**（既非 PASS 也非 FAIL，是 UNMEASURED）。无 `rust-toolchain.toml`，工具链 rustc 1.94.0。
- 工具链 remediation 选项（记录，非本次范围）：runner test target 的 `atomic_try_update` 要么换稳定 API，
  要么对整个 crate 启用 nightly（需单独评估，不在 Slice 2A scope）。

---

## 7. 边界（接手 agent 必须守）

- 不要启动 Slice 2B。
- 不要碰 LSP / plugin / detached / approval 面。
- 不要扩大 scope、不要重构 F2、不要改 catalog 语义。
- 不要 amend / squash / rebase / cherry-pick 历史 commit。append-only：`RED@SHA → fix → new SHA → 回归 → 独立评审`。
- **不要擅自 commit 那 380 行未提交改动**（非你写入；先与用户确认是合并、保留还是丢弃）。
- 不要宣布 Slice 2A PASS。

### 建议接手顺序
1. 先 `git status` + `git diff --stat` 确认 §1 描述的 380 行改动仍在且未被污染。
2. 审阅那 380 行改动（ISSUE A/C），跑 `cargo test -p webcodex-workspace --lib`（预期 88/12）确认 verdict gate 不退化。
3. 向用户确认两件事：ISSUE C 是否终态为 OPT-2；那 380 行改动是保留并 commit 化，还是丢弃。
4. 获得授权后，把 380 行改动 commit 化为新 SHA（不在本 handoff 分支，回到 `impl/webcodex-p1b-normalization-slice1`）。
5. 交 fresh independent Codex reviewer。

---

## 8. 证据分类

- **A（本 handoff 作者本轮现场核验）**：所有 SHA、git log、`git worktree` 状态、`ls-remote` 概念、文件改动清单、
  §1 的 380 行改动事实与内容、`cargo fmt/diff --check` 结果、`cargo check` runner 编译/测试态、rustc 版本。
- **B（从 0f56dd81 代码与当日 memory 日志读取）**：§3 的 5 点、§5 的 P1/P2/P3、§4 的 88/12 拆分、runner 限制。
- **C（仅从早期会话摘要记得，未本轮复验）**：round 1-2 的原始 task card 措辞、四轮 REJECT 措辞、F1/F2/F3 finding 标签。
  **C 类视为未证实**，接手前不要据其行动，先读仓库。
