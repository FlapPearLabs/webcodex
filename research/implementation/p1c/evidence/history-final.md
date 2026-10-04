# P1C evidence provenance and failed iterations

结果只归属其 source hash、实际命令、平台、toolchain 与退出状态，不能从一个绿色 wrapper 推导内部安全断言通过。

## 封存原语与架构反证

S1 prototype 有历史 source `06f810...` 与后来 `974677...` 两个证据时点，不互换。它没有 production consumer，两个自有文件经 hash guard 撤回，完整 reviewed patch 已封存。当前 production broker 是 accepted P1B 字节 `2894a0...`。本目录 setsid 记录是在 prototype 仍存在时运行；复跑须在 scratch exact base 应用 sealed patch，再使用存档固定 fixture。直接对当前 production crate 编译该 fixture 将找不到实验 API，不能声称已在 sealed 后重新跑过。

setsid 与 FD fixture 退出 0 表示探针及清理完成，实测内容反驳设计不变量，**不表示安全 PASS**。FD fixture 是临时空 writer capability，未读取真实 secrets。group escape 不是 Seatbelt escape；没有凭该现象声称脱离 kernel 文件/网络限制。

## 远端请求 harness

初始 harness 出现过 inventory pagination 字段缺失、wire 兼容字段不相等、Workflow Session 格式无效以及重复/并行投递顺序错误。它们属于 harness 构造失败，不是新生产入口拒绝的证明。最终 fixture 必须收到两条合法真实 Runner 结果后检查 marker。

第一次旧路由 wrapper 将预期 unsafe marker 作为整体成功而退出 0；该记录仅为 diagnostic，不是 required actual assertion RED。最终同一 no-effect assertion 在 old-route binary 收到 model connect marker 后令 harness 非零，而 Runner 正常退出，才可作为真正负对照。

## 结构门禁

早期 scratch 复用 target/cache，guard 的编译时 `CARGO_MANIFEST_DIR` 仍指主树，same gate 没有看见 inserted launcher；`same_gate_expected_raw_drift_red=false`，无区分力，不能计 NC 通过。最终用独立 scratch target 重建 guard，使相同 production scan 真正扫描 mutant scratch，新增 Command::new 与 spawn 两行必须导致 Cargo 非零。恢复 source exact bytes 后还需绑定未退休其他 origins。

## 文档/源冻结修正

独立 Standards 找出 Windows TOML path escaping 缺陷及额外 mixed-test 删除。源修正保留 local Bash rejection，只退旧 remote half；所有整文件 origin hashes 必须重新绑定，不能把过期 binary/source 证据重命名成新时点。11 whole-test retirement 与这一个 partial-test disposition 区分记录。

前几个执行上下文曾重用通用 metadata 文件名；`remote-c2/evidence-history.md` 记录可辨识的覆盖，guard 执行者也报告 `final-source-sha256.txt` / `final-scratch-manifest.txt` 等文件可能覆盖旧内容。不存在的旧版本不能恢复，本记录不声称完整保存了每次失败的全部 raw metadata。保留下来的失败原件与最新独立唯一命名证据分开，最终 source/exit 证明不依赖被覆盖的版本。

## 证据等级

普通 production build/native Runner requests 与 bootstrap test-target DIAGNOSTIC 分开。local fallback 若没有 local payload/profile-init marker positive control，只是 SOURCE_DERIVED_ONLY；未执行的 real remote、Linux/Windows、八项 restart、whole-family、PID reuse、output continuity 都是 NOT_RUN。ENV_BLOCKED/HOST_UNAVAILABLE/NOT_RUN 永不作为 PASS 或 migrated 数量。

## 最终源的窄清理

退休留下的 symlink 测试导入与 ssh.rs 空行由 Luna 清理。一次直接 edition 2024 格式化产生了相邻导入/B22 测试排版变化，主责拒绝该 diff；依据已冻结 scratch 哈希全部撤回，最终 persistent_shell 恢复 a501，ssh 相对 e672 只余两处已授权清理。当前最终候选另有 source-freeze，不将同目录后续 metadata 当成旧 NC 运行时 source snapshot。

旧 native contract binding 绑定 368589...，后来明确 AND 负对照与 fallback evidence 语义后的当前 contract 为 ab3236...。旧 binding 作为历史保留，不声称签署修订版；当前两个独立 review 从当前 contract 和最终 source/evidence 重新评估。

## 格式化残留的最终核验

前一摘要称 macOS child 文件的排版差异是 preexisting，actual exact-base cargo fmt exit0/current exit1 推翻了它。执行者随后确认 direct edition2024 formatter 递归改动该子模块，依据 preformat scratch 和 base 的共同 e498 哈希，在已知 e47e 当前字节上精确撤回该 owned diff。最后 cargo fmt --all --check 和 git diff --check 都 exit0，C2生产源、fixture与overlay pins不变。5596 source-freeze 记录是修复前的历史时点，不能改写为最终源；新的冻结清单排除这项额外改动。

旧 guard summary 的 scoped format PASS 与同目录 raw exit1 存在矛盾，最终不采该摘要为证据。新的 native owned-only rustfmt 明确 skip_children=true，实际exit0；撤回递归残留后的全库fmt另有独立命名原始0。
