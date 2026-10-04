# C2 Windows mixed test — scope correction

2026-10-04。Independent Standards 在 final source 预审中发现原 `windows_named_ssh_resource_routes_remote_without_changing_local_powershell` 也被删除，但它不在已签署的 11 个 whole-test retirement 清单内。

主负责人核读 exact baseline `f58e65c6d95bbd91165e97b4a98de694f97ae872` 的 persistent_shell.rs:1889–1931：第一段调用真实 handler，要求 local explicit Bash 在 Windows 被 `persistent_shell_dialect_unsupported` 拒绝；第二段要求旧 missing SSH resource 路由进入 remote preparer 并返回 `ssh_persistent_shell_spawn_failed`。后者依赖当前已经关闭的 backend，前者仍有效。

最小处置：恢复该函数的 local half，改名 `windows_local_bash_override_still_refused`，constructor 仅适配当前 `new(&shell)`，保留 active_count=0。仅退休 remote half，不恢复不再使用的 ssh_request adapter，不新增 local 行为。最终账本是 11 个 whole dedicated tests retired，加一项 mixed test 的 remote half retired / local half retained；不能说额外第十二个 whole test获准删除。

修改前实际只读 CodeGraph：impact 列该 test 自身一项，callers 无，callees fixture/default/new/request/handle/ssh_request；affected 是文件级关联测试提示，索引有同名 symbol 噪音，不能替代 exact source。无 production caller、权限契约或生产函数体改变。整文件 production-origin hash 仍需重新绑定；原生 binary 与最终候选 evidence 不得沿用过期整文件 hash。

此修正由 Luna 执行、Sol 独立复核；本文件为主负责人明确处置，不伪称 Windows native runtime PASS 或完整 P1C 签署。Windows runtime 保持 NOT_RUN。
