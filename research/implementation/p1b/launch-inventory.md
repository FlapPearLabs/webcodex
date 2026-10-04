# P1B launch inventory

This is a readable view derived from the sole accepted source, [`launch-inventory.json`](launch-inventory.json). Classification authority: GPT-6.1 Sol Max. The JSON is bound to implementation source HEAD `5ae81148f3943982dfcd1814e1083f947b00a323` and SHA-256 `d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee`. Fingerprints use `rust_syn_quote_tokens_v1`.

The JSON records 28 Rust production targets, 260 grouped raw rows / 290 occurrences, 30 references, 10 boundary body fingerprints, 128 non-Rust release assets, and 626 production source files in the conservative origin closure. Raw/reference occurrences and infrastructure forwarding rows are evidence anchors, not extra logical child launches.

| Class | Logical launch surfaces | Reading |
|---|---:|---|
| A | 4 | Existing registered-workspace execution authority and approved routing. This classification does not itself certify every runtime gate. |
| B | 22 | Model-triggered provider, external service, native application, SSH, project-creation, worktree, and plugin execution authority. Keep the exact operator/provider/project/network/credential recipient and lifecycle contracts in each JSON row; do not silently recast these as ordinary registered-workspace children. |
| C | 9 | Persistent sessions and detached supervisor/payload/watchdog launches with durable lifetimes. A later design must preserve reconciliation, Runner restart/orphan handling, process-group and FD ownership, completeness, and uncertain outcomes. P1C migration has not started. |
| D | 75 | Fixed host-owned probes and other launches with fixed purpose/payload. Four fixed probes can be triggered by model requests; that trigger is recorded separately from model authority to choose an executable or payload. |
| E | 13 | Operator/build-only launch surfaces with explicit host-side entry points. |

The accepted totals are 123 logical launches, 35 model execution-authority launches (A+B+C), 4 model-triggered fixed control probes, 39 total model-triggered launches, and 84 operator/build-only launches. Twenty infrastructure anchors are tracked separately and are not added again to logical launches. Eighty-nine non-process anchors in the JSON describe related source evidence, not launches.

For B, the precise follow-up capability and lifecycle obligations are row-specific in `next_contract`; recurring requirements include explicit provider/SSH/browser/native-application authority and credential recipient, with no authority inferred from cwd. For C, design durable/session authority and supervision before migration; do not drop restart survival, identity reconciliation, FD/process-group ownership, or uncertainty handling. These are accepted classifications and follow-up contracts, not claims that B/C confinement is already implemented.

The guard compares production raw sites, references, boundary body fingerprints, non-Rust release assets, and the exact path/bytes/target set of all 626 production source files. The source-origin check is deliberately conservative: any new or changed production source file, including a caller that reuses an existing D/E helper without adding a launch primitive, requires review. It is a source-change gate, not a semantic call graph. Guard tests, fixtures, and this documentation are outside that production closure.

## Validation evidence

The classification status `SOL_REVIEWED_CLASSIFICATION_ACCEPTED` and implementation validation status are separate. The candidate Rust gate and native harness results are recorded in [`evidence/`](evidence/) and summarized in `evidence/manifest.json`; the current evidence includes the exact-JSON guard comparison, compiled source-origin mutation/restore negative controls, native API alias detection, direct boundary caller drift detection, npm publication overlays, and native macOS runs against temporary projects and loopback services. The guard default lane passes 15 tests and ignores two explicit negative controls; the npm pack lane and held-grandchild deadline lane run separately. The npm lane proves publication behavior on the recorded macOS host only. The held-grandchild case proves deadline return only; it does not prove full process-tree cleanup.

The checkout evidence records actual Git index checkouts with `core.autocrlf=false` and `true`: both matched all 626 production origins and all 128 non-Rust assets. The approved `.gitattributes` and checkout matrix are hash-bound in `evidence/manifest.json`. This is Git checkout reproducibility evidence, not a Windows native runtime result. The workspace dependency gate also records the exact-START pre-existing allowlist failure and the Sol-approved single normal-dependency allowlist repair that now passes the official gate.

G current acceptance: [P1B acceptance and evidence index](acceptance.md) records the exact G identities, scoped results, and limitations. The independent Spec/security/architecture review is `PASS_SCOPED`; independent Standards review is `PASS`.

C closeout contract: raw evidence and exact-G reviews are archived byte-for-byte with provenance in [evidence/manifest.json](evidence/manifest.json). The exact-C decision, verified path set, and C SHA belong in the external exact-SHA closeout record; this navigation page does not claim a C signature.
