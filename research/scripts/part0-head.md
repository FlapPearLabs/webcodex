ROLE = INDEPENDENT_ADVERSARIAL_ARCHITECTURE_REVIEWER
MODE = READ_ONLY, INDEPENDENT, ADVERSARIAL
STANCE = DO NOT FLATTER. DO NOT DEFEND THE SUBMISSION. FIND DEFECTS.

You are reviewing a research/design submission. You are NOT the author. You did
not write any of it. Your job is to find what is wrong, unsupported, missing,
over-engineered, or mis-cited.

IMPORTANT ENVIRONMENT NOTE: your filesystem read tools are broken in this build
(`sandboxCwd must be an absolute file URI`). Therefore the entire submission has
been pasted below verbatim. Review ONLY the material below. Do NOT attempt to
read files from disk. Do NOT ask for files. If something you need is absent,
record that as a finding ("not submitted / cannot cross-check").

================================================================================
PART 0 — WHAT THE TASK WAS
================================================================================

Objective: make WebCodex (an MCP bridge that lets ChatGPT Web drive a local
project) into a safe, low-friction, Codex-style local coding agent.

Deliverables requested: four to six research documents covering
(1) security invariants, (2) a component reuse matrix, (3) a reference
architecture, (4) a minimal implementation plan, (5) security test vectors, and
(6) an OSS evidence ledger.

Hard constraints the author was given:
- Research first. No product code in this stage.
- Reuse mature OSS implementations rather than inventing a framework.
- Distinguish a POLICY engine from an OS SANDBOX; never conflate them.
- The model must not be able to grant itself higher privilege.
- A tool argument such as `approved=true` must never count as human approval.
- Danger mode must not be reachable from the remote agent.
- The workspace sandbox must cover the whole process tree.
- Prompt-injection detection is NOT a security boundary.
- Hard DENY must not be overridable by an auto-reviewer.
- Prefer an adapter or a few dozen lines of glue over a new framework.
- If the original plan was wrong, change the plan.
- Do not guess repository identities (several project names collide).
- Record evidence as repo + commit + file + symbol.

================================================================================
PART 1 — RAW VERIFIED FACT LIST (for cross-checking claims below)
================================================================================

These are the raw facts the author actually observed on the host. Use this list
to check whether the documents over-claim beyond it. Treat these as given; if a
document asserts something NOT derivable from this list, that is a finding.

[Environment]
- Host: macOS (Darwin, arm64). Local HTTP proxy present; GitHub reachable.
- Tools: gh CLI 2.89.0 (authed as FlapPearLabs); codex-cli 0.136.0; hermes v0.21.5;
  cargo/rustc 1.95.0 (present, not on PATH); node 22.22.2; python 3.13.12.
- codex-cli 0.136.0 read-only sandbox CANNOT read files on this host:
  "codex/sandbox-state-meta: sandboxCwd must be an absolute file URI: relative
  URL without a base". (Hence this pasted-prompt review.)

[Repos and identities, verified via gh / clone]
- WebCodex: github.com/yyjeqhc/webcodex, owner yyjeqhc, Apache-2.0, Rust, 2015 stars,
  pushed 2026-09-28. HEAD 7301186b98527cb4ebc0191f6033474e9abf7c20.
  Fork created: github.com/FlapPearLabs/webcodex (isFork=true, parent yyjeqhc/webcodex).
- Codex: github.com/openai/codex, Apache-2.0, Rust, 126943 stars.
  Clone HEAD 69f7140559180269e2eb8f5be6e0c20eb37b0c85.
- OpenCode: `gh repo view sst/opencode` RESOLVED TO anomalyco/opencode (owner changed).
  MIT, TypeScript. Clone HEAD 3c893f0a166cfc433819b4eff65d2e6c7696a1c9.
- Hermes: github.com/NousResearch/hermes-agent, MIT, Python. Local worktree HEAD
  79dbb1450ec404a2240529f5554526da4cfec498 (v0.21.5+3172.g79dbb14). User has forks
  FlapPearLabs/hermes-agent and panglihaoshuai/hermes-agent.
- Pi: `gh repo view badlogic/pi-mono` RESOLVED TO earendil-works/pi (owner AND repo
  name changed). MIT, TypeScript, 110033 stars. Clone HEAD
  11894012dd461232eb075bc890538b6866860a10.
- "Pi" is a crowded namespace: a repo search also returned
  Dicklesworthstone/pi_agent_rust, smallnest/pigo, vastsa/PI-Desktop,
  abcwyc/pi-agent-desktop, Pinvou/pinvou-agent — none of which is the P1 target.
- GrokCode and zcode were NOT researched (name collisions unresolved; P2).
- DesktopCommanderMCP-hardened `origin` = github.com/wonderwhy-er/DesktopCommanderMCP
  (a THIRD-PARTY upstream, not user-owned). It was explicitly out of scope.

[WebCodex source observations]
- Unified execution enum: crates/webcodex-core/src/runner_operation.rs:632
  `enum RunnerOperation` with variants RunShell, RunProcess, RunScript,
  RunInternalPosixScript, RunSkillResource, Job, File, Project, Computer, Browser,
  PlanProjectValidation, Validation, Lsp, PersistentShell, McpGateway,
  PluginGateway, CodingAgent, Skill, SshResource, RunnerConfig, RunnerInstruction.
- Single spawn chokepoint: crates/webcodex-process/src/unix.rs:43
  `ManagedChild::spawn` -> `spawn_with_options`: `command.process_group(0)` then
  `command.spawn()` at line 57. Windows uses a Job Object with
  JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE. Crate doc: the owned entity is the TREE.
- Counts: 38 textual `ManagedChild::spawn` hits; after removing tests/comments,
  17 production call sites. ZERO direct `.spawn()`/`.output()`/`.status()` calls
  inside crates/webcodex-runner/src.
- Policy layer: src/tool_runtime/permissions/{model,policy,evaluator,risk,mod}.rs
  * model.rs:18 `enum AuthorityMode { TrustedAgent, Restricted }`; default TrustedAgent;
    env WEBCODEX_AUTHORITY_MODE; legacy WEBCODEX_PERMISSION_MODE.
  * policy.rs:64 EffectiveAuthorityConfig::from_env; policy.rs:152-165 fail closed on
    InvalidMode (human_approval_required=true, auto_authorize=false).
  * policy.rs:234 authority_profile_payload_for: project_write/shell/git/network/
    package_install/service_control all == auto; release == "user_task_scoped".
  * mod.rs:124 is_hard_denied_output: structured kinds policy_rejected,
    session_guard_denied, unknown_session_id, session_project_mismatch,
    confirmation_required, job_not_found, job_project_mismatch, job_stop_forbidden;
    PLUS substring matches on error PROSE: "sensitive path", "sensitive artifact path",
    "path must be project-relative", "path cannot contain parent traversal",
    "absolute paths are not allowed", "path traversal".
  * mod.rs:156 permission_summary_from_events counts auto_approved/approved/denied/
    expired/requested/hard_denied.
  * Gate call sites: dispatch.rs:1886, coding_task.rs:689 & 815, specialized.rs:529.
- crate webcodex-core/src/workflow_session_contract.rs:503 `enum PermissionOutcome`
  = AutoApproved|Approved|Denied|Pending|HardDenied (wire: "auto_approved",
  "approved", "denied"/"expired", "requested"/"pending", "hard_denied").
  :538 `struct PermissionDecision { required, policy, request_id, status, reason,
  risk, tool_name, project }`.
- NO OS SANDBOX: grep for seatbelt|sandbox-exec|landlock|bubblewrap|seccomp over
  src/, crates/, docs/ returned ZERO matches.
- NO APPROVAL FLOW: docs/agent/permission-model.md §2 says restricted DENIES and
  "There is no separate Connector command-approval queue or host-side task approval
  namespace"; §8 non-goals "No approval UI or notification system".
  docs/agent/architecture-decisions.md:375 says core execution "cannot depend on
  Apps, MCP Tasks, MRTR, elicitation, progress extensions, or iframe state".
  src/mcp/protocol.rs:106 legacy_initialize_payload advertises ONLY
  capabilities:{tools:{listChanged:false}}. MCP_INFO_METHODS (protocol.rs:14) has no
  elicitation method. crates/webcodex-cli has NO `approve` or `deny` command.
- Tool contracts: crates/webcodex-tool-contracts/src/metadata.rs:66
  `enum ToolApprovalPolicy { None, Standard, InheritFromStart, Unknown }`;
  :2 `enum ToolRisk { Read, ProjectWrite, SkillManage, MemoryManage,
  CommunicationManage, SessionCollaborate, WorkflowManage, CheckpointManage,
  RunControl, ComputerControl, BrowserControl, JobRun, Unknown }`;
  :43 `enum ToolEffect { Observe, Mutate, Execute, Unknown }`.
- Control-plane spawns (bootstrap/tunnel, not model-reachable) live in
  src/project_entry*.rs and src/server_listener.rs:370.
- Prior empirical baseline (a separate, already-adversarially-reviewed document)
  recorded under the default mode: `cd /tmp && cat <outside>` ALLOWED;
  `ls ~` ALLOWED; `curl https://example.com` ALLOWED with no approval;
  structured-surface absolute-path and `..` probes REJECTED.

[Codex source observations]
- codex-rs/sandboxing: sandboxing/src/{manager.rs,seatbelt.rs,bwrap.rs,landlock.rs,
  windows.rs,windows_mxc.rs,violation.rs,spawn.rs}. LOC: seatbelt.rs 1125,
  manager.rs 816, violation.rs 300, spawn.rs 142 = 2383.
- manager.rs:49 get_platform_sandbox -> MacosSeatbelt | LinuxSeccomp |
  WindowsRestrictedToken. manager.rs:352 SandboxManager::transform REWRITES ARGV.
- manager.rs:434-471 macOS arm prepends MACOS_PATH_TO_SEATBELT_EXECUTABLE;
  seatbelt.rs:62 that constant == "/usr/bin/sandbox-exec".
- seatbelt.rs:336-346 distinct network rules (localhost:*, DNS "*:53", local ip
  bind); seatbelt.rs:374 open-network branch.
- sandboxing/Cargo.toml deps: codex-mxc-sandbox, codex-network-proxy,
  codex-protocol, codex-uds, codex-utils-absolute-path, codex-utils-path-uri,
  codex-utils-pty, codex-windows-sandbox, dunce, libc, serde_json, regex-lite,
  tokio, tracing, url, which.
- protocol/src/models.rs:422 `enum PermissionProfile { Managed{file_system,network},
  Disabled, External{network} }`; :410-416 built-ins ":read-only", ":workspace",
  ":danger-full-access"; :471-480 Default = Managed{Restricted, Restricted}.
- execpolicy/src/decision.rs:9 `enum Decision { Allow, Prompt, Forbidden }`.
- protocol/src/protocol.rs:986 `enum AskForApproval { UnlessTrusted, OnRequest
  (default), Granular(GranularApprovalConfig), Never }`; :1012 GranularApprovalConfig
  includes `mcp_elicitations: bool`.
- protocol/src/protocol.rs:4159 `enum ReviewDecision { Approved,
  ApprovedExecpolicyAmendment{..}, ApprovedForSession, ApprovedMcpPolicyAmendment,
  NetworkPolicyAmendment{..}, Denied{rejection}, TimedOut, Abort }`; :4198
  `impl Default` == `Denied{rejection:"denied"}`.
- core/src/guardian/ dir 9527 LOC incl tests; decision.rs:44 doc comment "`None`
  requests the existing user flow. No contributor is never an implicit allow.";
  decision.rs:17 spawns the review on a dedicated thread "codex-approval-review";
  reviewer_config.rs:32 build_guardian_review_session_config resolves a separate
  reviewer model and injects host-controlled policy instructions.
- guardian-context/src/lib.rs:1-12 "Shared context sections for synchronous Guardian
  review and asynchronous scoring".
- ext/guardian-reviewer (package codex-guardian-reviewer) and ext/guardian-v2 are
  EXTENSIONS; guardian-reviewer depends on codex-extension-api, codex-prompts,
  codex-protocol, codex-mcp, codex-otel, codex-analytics, codex-feedback.
- crates.io checks: GET /api/v1/crates/codex-sandboxing ->
  {"errors":[{"detail":"crate `codex-sandboxing` does not exist"}]}.
  GET /api/v1/crates/codex-protocol -> exists, default_version "0.63.0",
  created 2025-12-11, 1667 lifetime downloads, num_versions 1 (stale).
- codex LICENSE = Apache-2.0; NOTICE = "OpenAI Codex / Copyright 2025 OpenAI"
  plus MIT-derived Ratatui code.

[OpenCode source observations]
- packages/schema/src/permission.ts: Effect = Literals(["allow","deny","ask"]);
  Rule = {action,resource,effect}; Ruleset = Rule[]; Reply =
  Literals(["once","always","reject"]); Request = {id,sessionID,action,resources[],
  save[],metadata?,source?}; Events permission.v2.asked / permission.v2.replied.
- packages/core/src/permission.ts (310 LOC): :15 missingAgentPermissions =
  [{action:"*",resource:"*",effect:"deny"}]; :76 evaluate uses `.flat().findLast(
  match) ?? {action,resource:"*",effect:"ask"}` (LAST MATCH WINS, default ask);
  :147 denied() fast path; :155 evaluateInput precedence deny > ask > allow;
  :197 assert -> BlockedError/allow/await deferred; :220-247 reply "reject" CASCADES
  to all other pending requests in the same session; :250-256 reply "always" persists
  via saved.add({projectID, action, resources: request.save}); :261-283 retroactively
  resolves pending; :119-129 finalizer fails all pending with DeclinedError.
- packages/core/src/util/wildcard.ts: string glob -> regex (escapes metachars,
  `*`->`.*`, `?`->`.`, anchored ^...$). NO path canonicalization.
- packages/core/src/permission/saved.ts + sql.ts: rows (id, project_id, action,
  resource); list filtered by project_id; add uses onConflictDoNothing.
- packages/opencode/src/session/tools.ts: plugin hooks "tool.execute.before"
  (receives and can mutate {args}) and "tool.execute.after".
- OpenCode has NO OS sandbox.
- OpenCode LICENSE = MIT, "Copyright (c) 2025 opencode".

[Hermes source observations]
- hermes_cli/approval_mode.py:19 VALID_APPROVAL_MODES = ("manual","smart","off").
  Module doc: approval mode is "profile-scoped configuration, not conversation
  state"; changing it "must not rebuild a live agent or mutate its system
  prompt/tool schema, preserving the prompt-cache prefix". Managed policy causes
  SystemExit -> cannot be overridden.
- hermes_cli/approval_transport.py (185 LOC): :27 ApprovalChoice =
  Literal["once","session","always","deny"]; :1-6 doc "Transports only present an
  immutable, redacted request and return a correlated human decision. They do not
  participate in command detection or authorization policy."; :55-74
  ApprovalRequest.create -> uuid4 hex id, digest = sha256(canonical json incl.
  session_key), allowed_choices from allow_session/allow_permanent,
  timeout default 300s; :31-38 ApprovalDecision{request_id, request_digest, choice};
  :99-101 _deny(); :114-116 BoundedSemaphore(8) -> busy => deny; :121-144 daemon
  thread worker, BaseException -> error; :145-161 deadline + interrupt -> deny;
  :167-185 _validate_decision -> failure codes timeout | error | invalid | stale,
  all returning deny.
- tools/approval_floors.py: floors run BEFORE yolo / approvals.mode: off /
  cron approve-mode; hardline, `sudo -S` password piping, user `approvals.deny`
  fnmatch globs; uses normalized/deobfuscated variants; message says it cannot be
  executed "not even with --yolo, /yolo, or approvals.mode=off".
- tools/approval.py (1363 LOC): :249 approve_session(session_key, pattern_key);
  :350 is_approved; :366 approve_permanent; :372 load_permanent; :448
  save_permanent_allowlist; :285 clear_session; :122/:138/:175/:209/:219/:225
  gateway notify/resolve/withdraw/ack/has_blocking/pending_count; :585/:618/:637
  unattended contexts deny; :761 _smart_gate; :793 _human_decision; :1052
  _floor_block; :68-93 denial breaker.
- tools/path_security.py:8 validate_within_dir uses
  path.resolve().relative_to(root.resolve()); :19 has_traversal_component;
  :26-37 rejects control chars incl. NEL/LS/PS.
- Hermes LICENSE = MIT, "Copyright (c) 2025 Nous Research".

[Pi source observations]
- packages/agent/src/types.ts:66 BeforeToolCallResult {block?,reason?,terminate?}
  ("Returning {block:true} prevents the tool from executing");
  :103 BeforeToolCallContext {assistantMessage, toolCall, args, context};
  :89 AfterToolCallResult field-level overrides.
- packages/agent/src/agent.ts:123,201,241,480 beforeToolCall?; agent-loop.ts:722
  invokes it.
- packages/coding-agent/src/core/agent-session.ts:533 installs
  agent.beforeToolCall -> runner.emitToolCall({type:"tool_call",...}); the catch
  clause throws "Extension failed, blocking execution: ..." => a failing extension
  BLOCKS. :551 afterToolCall -> emitToolResult.
- examples/extensions/dirty-repo-guard.ts: "if (!ctx.hasUI) { // In non-interactive
  mode, block by default  return { cancel: true } }".
- examples/extensions/timed-confirm.ts: ctx.ui.confirm(..., {timeout: 5000}).
- Import scope "@earendil-works/pi-coding-agent" confirms the rename.
- Pi LICENSE = MIT, "Copyright (c) 2025 Mario Zechner".
- No first-party sandbox/approval extension found in the ecosystem search.

[Scope decisions the author made]
- STOP_OSS_EXPANSION = YES (Codex + OpenCode + Hermes + Pi covered sandbox, policy,
  approval, reviewer, and extensions).
- Not opened: GrokCode, zcode, goose, aider, Cline, Roo Code, Claude Code clones,
  OpenHands, Daytona, E2B, standalone bubblewrap/Landlock/Seatbelt wrappers.
- No product code written. No compile spike performed.

[ROUND-2 SUPPLEMENT — corrections made after your Round 1 review]
The author accepted all twelve issues and both additional defects from Round 1.
These additional raw facts were established while responding, and you should
cross-check the revised documents against them.

1. CORRIGENDUM TO A PRIOR CLAIM. An earlier revision of the submission stated
   "zero direct .spawn()/.output()/.status() calls inside the runner". That was
   FALSE. It came from a grep invocation that silently matched nothing due to a
   shell glob-expansion failure (`--include=*.rs` under zsh), and a structural
   conclusion was drawn from the empty result. Re-verified direct
   `Command::spawn()` sites (non-test, non-fake):
     crates/webcodex-runner/src/webcodex_runner/remote_shell.rs:109   (unix branch)
     crates/webcodex-runner/src/webcodex_runner/ssh.rs:1142           (unix branch of spawn_piped_ssh_child)
     crates/webcodex-runner/src/webcodex_runner/detached_job.rs:1924  (supervisor attempt 1)
     crates/webcodex-runner/src/webcodex_runner/detached_job.rs:1941  (supervisor fallback)
     crates/webcodex-runner/src/webcodex_runner/detached_job.rs:2409  (payload; sets process_group(tree_pid) itself)
     crates/webcodex-persistent-shell/src/lib.rs:1965                 (unix; preceded by pre_exec at :1939)
     src/tool_runtime/helpers.rs:146                                  (server side; sets process_group(0))
     crates/webcodex-workspace/src/workspace_checkpoint.rs:358,398    (unclassified)
     crates/webcodex-workspace/src/project_context.rs:598             (unclassified)
     crates/webcodex-cli/src/webcodex_cli/connect/process.rs:568,655  (operator CLI)
     crates/webcodex-cli/src/webcodex_cli/controller.rs:767           (operator CLI)
     crates/webcodex-environment/src/{installer_unix.rs:442,upgrade.rs:2563,
       unified_update/installer.rs:491,process.rs:42}                 (operator CLI)
     apps/desktop/src-tauri/src/{updates/install/prepare.rs:118,162,244,325,
       updates/install/native.rs:127,418,platform/opener.rs:84,
       platform/windows.rs:43}                                       (desktop app)
   PLATFORM ASYMMETRY: remote_shell, ssh and persistent_shell use
   `ManagedChild` on Windows and a direct `Command::spawn()` on Unix. macOS —
   the primary target — takes the DIRECT path.
   Only two functions call Command::spawn() on behalf of the ownership
   abstraction: crates/webcodex-process/src/unix.rs:57 and windows.rs:168.
   ManagedChild::spawn* is called from ~20 non-test sites across 9
   crates/binaries. There is NO single spawn chokepoint.

2. POST-EFFECT vs PRE-EFFECT. dispatch.rs:1883-1912 carries the author's
   authoritative ordering comment: "Order: session/auth guards above →
   permission gate → mutation below. Path/sensitive hard checks still run
   inside tools; hard-deny filter suppresses permission attach so soft policy
   never overrides them." The permission gate at :1886 -> :1893
   (permission_execution_denied_result) IS pre-effect. is_hard_denied_output at
   :2006 runs AFTER execution and only filters the permission attach.

3. OPTIONS RECLASSIFIED AS "NOT ASSESSED" (previously closed):
   codex-process-hardening; codex-network-proxy as a reuse candidate;
   codex-sandboxing via a PINNED GIT dependency; a bounded VENDOR_SUBSET of the
   SBPL builder only.

4. REVISED NET-NEW-CODE ESTIMATE: ~4.1k-7.2k LOC, itemised to include spawn
   normalization, per-surface sandbox-plan propagation, control-channel
   isolation, and platform verification. (Previously ~3.0k-4.9k.)

5. UNSAFE ROLLBACK REMOVED: the proposed `WEBCODEX_SANDBOX=none` runtime switch
   is gone. No selectable pass-through backend ships. The only disable is a
   maintainer-only, unreleased build configuration.

6. A NEW UNVERIFIED CLAIM WAS DOWNGRADED: the assertion that WebCodex
   canonicalizes project roots and rejects outside-resolving symlinks came from
   a SEPARATE baseline document not submitted here, and was NOT re-verified. It
   is now marked UNVERIFIED_IN_THIS_RESEARCH.

7. Review modality (restated, unchanged): codex-cli 0.136.0 on this host cannot
   read files in read-only sandbox — error
   "codex/sandbox-state-meta: sandboxCwd must be an absolute file URI: relative
   URL without a base". Hence the embedded-prompt method.

[ROUND-3 SUPPLEMENT — corrections made after your Round 2 review]
Round 2 produced ACCEPT on checks 2, 3, 7, 9, 12; seven issues remained
(1, 4, 5, 6, 8, 10, 11) plus three revision-introduced defects and two
non-blocking weaknesses. Facts relevant to cross-checking the new revisions:

8. PER-OPTION REUSE VERDICTS. The blanket
   `VENDOR_SUBSET_VERDICT = NOT_PRACTICAL` and
   `DIRECT_DEPENDENCY_VERDICT = NOT_PRACTICAL` are WITHDRAWN. Current verdicts:
     DIRECT_DEPENDENCY via crates.io                = NOT_PRACTICAL (assessed)
     DIRECT_DEPENDENCY via pinned git               = NOT_ASSESSED
     VENDOR_SUBSET of the whole sandbox crate       = NOT_PRACTICAL (assessed)
     VENDOR_SUBSET of a bounded piece (SBPL builder)= NOT_ASSESSED
     SUBPROCESS_ADAPTER + small profile compiler    = RECOMMENDED
   No blanket NOT_PRACTICAL verdict is claimed.

9. SPAWN INHERITANCE. Confinement is inherited because the sandbox FACILITY is
   entered before exec. Process-group membership provides ownership for signal
   delivery and tree termination only; it provides NO confinement and is no
   longer cited as the inheritance mechanism anywhere.

10. NETWORK DESCRIPTOR DECISIONS (were previously deferred):
    - DNS default = DENY. Name resolution only via a locally-owned enforcing
      proxy parameterised by the request allow-list, or not at all.
    - If no local enforcing proxy exists, the enforceable descriptor is IP:port
      ONLY, and hostname-scoped grants are NOT OFFERED (an unenforced hostname
      allow would be a false guarantee).
    - Localhost grants must be port-scoped and must never be phrased as
      "localhost allowed"; the control-plane channel is unconditionally denied.
    - SSH: confining the local ssh CLIENT does not constrain the REMOTE host.
      `exec.remote` is its own capability; remote execution is outside the
      boundary and must not be described as sandboxed.

11. STRUCTURED PATH RULE = ONE RULE. Absolute paths are REJECTED on the
    structured tool surface, including in-project absolute paths. No
    "allow-if-it-resolves-inside" special case.

12. HARD LINK RESOLUTION: PE-06a agent-created hardlink = strict DENY;
    PE-06b pre-existing in-workspace hardlink = KNOWN_LIMITATION (excluded from
    OUTCOME_STRICT); PE-06c opt-in `require_inode_allowlist` moves PE-06b to DENY.

13. SAME-USER CONTROL CHANNEL. New §10.4: a model-spawned process runs as the
    same host user, so a channel guarded only by file permissions is
    insufficient. Four candidate boundaries named (sandbox denial of the
    channel; unix-socket peer-credential check against a control-plane pid
    allow-list; never-exposed single-use token verified constant-time;
    OS-mediated dialog). At least one must be implemented and tested.

14. "Escape is impossible" was removed from P1; P1 now states a qualified
    guarantee plus the hardlink limitation as part of acceptance.

[ROUND-4 SUPPLEMENT — corrections made after your Round 3 review]
Round 3 produced ACCEPT on checks 2, 3, 7, 8, 9, 12. Six issues remained
(1, 4, 5, 6, 10, 11) plus two new defects.

15. COMPONENT_REUSE_MATRIX §2 OPENING SENTENCE rewritten. It no longer asserts a
    blanket NOT_PRACTICAL. It now reads: SUBPROCESS_ADAPTER is the recommended
    route; DIRECT_DEPENDENCY via crates.io and VENDOR_SUBSET of the whole crate
    are assessed as not practical; pinned-git dependency and bounded-subset
    vendoring are NOT ASSESSED and are not closed; no blanket verdict is claimed.

16. I4 (SECURITY_INVARIANTS) rewritten. Two mechanisms are now separated in an
    explicit table: TREE OWNERSHIP (process group / Windows Job Object) provides
    signal delivery and guaranteed termination; TREE CONFINEMENT requires the
    sandbox facility to be entered before exec. ManagedChild supplies only the
    former. Confinement does not follow from process-group membership.

17. §11 (REFERENCE_ARCHITECTURE) "new session" claim replaced. Now distinguished:
    - setsid()/process_group()/Job-Object breakaway escapes the PROCESS GROUP
      => defeats tree ownership; does NOT by itself remove an applied sandbox
      profile (a Seatbelt policy cannot be un-applied from inside).
    - Escaping the FACILITY is facility-specific (e.g. unprivileged user
      namespace creation on Linux, an unsandboxed helper, a platform service
      acting for the child) and must be enumerated PER BACKEND and tested.
    The document no longer asserts that a new session escapes confinement.

18. NETWORK TEST TABLE now has three columns: (A) AUTO without an enforcing
    proxy, (B) AUTO with one, (C) network off. Without a proxy NET-01, NET-03,
    NET-05, NET-06, NET-07 are DENY. ASK appears ONLY in column B. Rationale
    stated: an approval that cannot be enforced is a false guarantee.

19. PE-06 REASONING REPLACED. Two prior justifications withdrawn with reasons:
    (i) hardlink creation needs write access to the DESTINATION directory, which
        is the writable workspace, so an outside-inode link IS creatable;
    (ii) a device+inode ALLOW-list cannot distinguish two names for one inode.
    Replacement, ownership-based:
      PE-06a  outside file NOT owned by the sandboxed identity  -> DENY
              (kernel fs.protected_hardlinks / ownership)
      PE-06a' outside file OWNED by it (the normal developer case) ->
              KNOWN_LIMITATION; real mitigation is UID SEPARATION
      PE-06b  pre-existing in-workspace hardlink -> KNOWN_LIMITATION
      PE-06c  require_uid_separation = true moves all of PE-06 to DENY
    `require_inode_allowlist` no longer exists as a concept.

20. OSS_RESEARCH_EVIDENCE §1: the ACTIVITY column (which said "active") is
    replaced by SOURCE_READ_AT, the commit actually read. "active" was a
    maintenance judgement the facts do not establish.

21. P8 (IMPLEMENTATION_PLAN) is no longer called independent/low-risk/omissible.
    It is the phase that closes the I17 gaps (plugin argument mutation, plugin
    execution authority, result laundering of hard denials). Until P8 lands,
    plugins must be DISABLED rather than permitted under an undefined contract.

22. NET-11 corrected. It no longer argues that a read denial makes network
    exfiltration moot. New position: the workspace is readable BY DESIGN, so
    exfiltration of WORKSPACE content over an allowed channel is ACCEPTED; the
    actual control is the independent denial of CREDENTIAL content. Nothing
    stronger is claimed.

23. OSS_RESEARCH_EVIDENCE §8 expanded to 10 numbered limits, now including:
    spawn reachability is a judgement not a proven call graph (2 sites
    unclassified); the single-uid hardlink limitation; network enforcement is
    design-level; codex-process-hardening unread.

[ROUND-5 SUPPLEMENT — corrections made after your Round 4 review]
Round 4 produced ACCEPT on checks 1, 2, 3, 5, 7, 9, 10, 11, 12. Three issues
remained (4, 6, 8) plus two new boundary contradictions.

24. I4: "guaranteed termination" REMOVED. The ownership row now reads
    "best-effort termination", and explicitly names the defeat mechanisms
    (setsid(), process_group(), Job-Object breakaway). It also notes that
    WebCodex's own remote_shell/ssh/detached_job Unix paths already spawn
    outside ManagedChild, so this is a live property of current code, not
    hypothetical. Confinement is still stated as a separate mechanism.

25. PE-06c PRECISION ADDED. Narrowed claim:
    - mode-protected outside inode (0600 credential files): uid separation DOES
      deny it, because access to the INODE is denied. This is the
      security-relevant case.
    - world-readable outside inode: a different uid can read it through the
      link, but could equally read it via its original path, so there is NO
      INCREMENTAL EXPOSURE. The link is not control-relevant.
    - The guarantee is restated as "no additional access beyond what the
      identity already had", which is the property that actually holds.
    - macOS has no fs.protected_hardlinks equivalent, so uid separation is the
      only mechanism covering PE-06a'.

26. PLUGIN DISABLE GATE ADDED TO P0. P0 acceptance criterion (6) now requires a
    plugin/hook disable gate with plugins DISABLED BY DEFAULT until P8 defines
    the contract. P0's rollback explicitly forbids reverting that gate. P8's
    ROLLBACK corrected: rolling back P8 must leave hooks DISABLED, not "restore
    current behaviour", because current behaviour is the undefined plugin path
    that I17 says must not run.

27. SECRET BOUNDARY MADE EXPLICIT (was circular). The NET-11 note no longer
    relies on "credentials are denied". Two disjoint sets are now defined, each
    with its own deny rules:
      SET 1  project tree MINUS secret patterns            -> readable/writable
      SET 2  secret patterns INSIDE the project tree        -> DENIED by explicit
             rules in the sandbox profile (.env, *.pem, id_rsa, .npmrc,
             .git/config, cloud credential files, agent token stores)
      SET 3  well-known credential locations OUTSIDE it     -> DENIED (~/.ssh,
             ~/.aws, ~/.config/gh, keychains, agent token stores)
      SET 4  control-plane channel                          -> DENIED
    A profile granting "read the project root" does NOT satisfy the row.
    The pattern list is operator-extensible and its INCOMPLETENESS IS STATED as
    a documented limitation. I16 therefore remains PARTIAL, not HOLDS.
    P1b acceptance criteria (9) and (10) were added.

28. CONTROL-PLANE REACHABILITY DOWNGRADED. REFERENCE_ARCHITECTURE §4 and
    OSS_RESEARCH_EVIDENCE §2.5 no longer assert "not model-reachable". They now
    state that reachability from a tool call is the INTENT and is NOT PROVEN;
    that src/tool_runtime/helpers.rs:146 was classified model-reachable; that
    the call graph is unproven; and that no document may make the categorical
    claim until the P0 spawn inventory and the CP-04 manifest test establish it.

[ROUND-6 SUPPLEMENT — corrections made after your Round 5 review]
Round 5 produced ACCEPT on checks 1, 2, 3, 4, 7, 9, 10, 12. Four issues remained
(5, 6, 8, 11) plus two new defects and two non-blocking weaknesses.

29. NEW PHASE P1c ADDED — "Network descriptor and enforcing proxy". The gap was
    real: no phase implemented an enforceable network destination, so a network
    ASK had no execution path. P1c has its own files, scope, tests, acceptance
    criteria and rollback. Scope is IP:port descriptors FIRST; hostname-scoped
    grants are enabled only once a locally-owned enforcing proxy is proven to
    filter by name, and are NOT OFFERED before that. Hard gate: NO PHASE MAY
    ADVERTISE A NETWORK ASK UNTIL P1c CRITERIA 1-5 PASS; until then network-
    capable capabilities resolve to DENY under every mode, including AUTO and
    APPROVE_FOR_ME. IMPLEMENTATION_PLAN P5 acceptance criterion (6) encodes this
    gate. P1c's rollback is explicitly non-permissive: rollback returns the
    system to network DENY.

30. PE-06 NOW HAS FOUR CASES AND TWO MECHANISMS. The blanket claim that
    require_uid_separation = true "moves the whole of PE-06 to DENY" is
    WITHDRAWN. Mechanisms:
      require_uid_separation        closes PE-06a' and PE-06b (mode-protected
                                    inodes) by denying access to the INODE
      deny_multiply_linked_inodes   a DENY-based st_nlink > 1 check; closes
                                    PE-06c and PE-06b generally. Unlike an
                                    allow-list it does NOT need to distinguish
                                    two names for one inode.
    PE-06c is the case where the profile denies an ORIGINAL PATH but the same
    world-readable inode is reachable through an allowed in-workspace path;
    uid separation does NOT close it, the link-count check does.
    OUTCOME_STRICT now excepts only PE-06a'.

31. PLUGIN GATE IS NOW A HARD GATE. P0 criterion (6): no configuration, flag,
    or tool call may enable plugin hook execution before P8's contract exists.
    ("disabled by default" was too weak.) P8 NOW DEPENDS ON P1b, not P0,
    because P8 requires the plugin process to be sandboxed before it may run.
    The sequencing graph and P8 note were corrected.

32. OSS_RESEARCH_EVIDENCE §2.5 categorical sentence REMOVED. It no longer says
    W-57..W-63 are "reachable only from CLI/bootstrap flows, not from the MCP
    tool surface". It now says those rows establish WHERE the spawns are and
    what they launch, NOT that they are unreachable; reachability from a tool
    call is unestablished for all of them and is a P0 task verified by CP-04.

33. ATTACH-SURFACE WORDING MADE CONSISTENT. REFERENCE_ARCHITECTURE §2 diagram,
    §6, §11 diagram, §16 and COMPONENT_REUSE_MATRIX row 7 no longer say the
    sandbox is attached "at ManagedChild::spawn" alone; they describe the attach
    surface as the PAIR {managed spawn path, shared hook}, matching §4.

34. SECRET BOUNDARY: I16 remains PARTIAL and the pattern list is stated as
    incomplete and operator-extensible. It is not presented as protection for
    arbitrary project credentials.

================================================================================
PART 2 — THE SUBMISSION (verbatim)
================================================================================

<<<SUBMISSION_START>>>
