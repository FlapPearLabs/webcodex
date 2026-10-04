# C2 native evidence 合同独立核读

独立 Sol Spec/security，2026-10-04；只签验证方法，不签未运行/未冻结实现 PASS。核读 `C2-NATIVE-EVIDENCE-CONTRACT.md` 后，方向为 **ACCEPTED_WITH_PRECISE_EVIDENCE_REQUIREMENTS**。

普通无 bootstrap/额外feature/shim 的 dev Runner 与精确 source/Cargo.lock、实际 compiler/platform/build exit/binary hash 绑定，是生产路径拒绝证据的适当入口。loopback synthetic register/poll/result、有效临时 project/resource、子进程独立 env、marker fake SSH 不接实际host/credentials、固定控制面 -V 探针单列、finally bounded exact owned cleanup，均在本次授权生命周期验证范围内。fake SSH 是 dependency fixture，不是改生产 Runner 的诊断实现。

必要的两项收紧：

1. 原反证段的“模型 marker 必须出现，或 fixed refusal/no-effect 必须真实非零”不得解释成任选其一。必须先 collect result/marker，再实际观察 exact-base 旧 remote 路径的模型 prepare/connect/bootstrap marker 至少一项非零，并由同一模型 no-effect 行为断言使 gate actual exit 非零。capability 或错误码差异不能先中断收集或代替实际 marker；compile failure、host unavailable、fixture failure、timeout不算RED。该合取用于证明同一fixture识别得到真实旧生产效果。
2. 新生产 Runner 的 SSH marker 为零，不能单独证明没有 local fallback 效果。若要将“无本地fallback”列为 native 实际观测，fixture应使用临时 local payload/profile-init marker，在open/exec全部相关phase确认它也为零。若该marker未实际覆盖或验证，应诚实标此项为source-derived，而不是native观测PASS；fresh source审查仍应确认拒绝发生于所有local/remote执行分支之前。

omitted-resource 已有 remote identity 的写入隔离，由同源 SpyTransport 与实际移除 fence 的 mutation RED支持；新生产 Runner不能创建这种legacyentry，测试专用状态注入不能包装成native legacy recovery。该边界准确。

实际 native 当前平台只可声称 C2 launch/exec 被关闭；不得提升为 Windows/Linux host PASS、remote-host confinement、detached全族终止、restart recovery、C7完成或整个P1C完成。实际全部source/build/behavior/NC/raw由执行上下文产生，fresh独立review对最终freeze验收。本旧架构上下文不预签其实现结果。
