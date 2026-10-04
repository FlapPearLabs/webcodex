# C2 final-source native evidence — 2026-10-04

The candidate freeze JSON hash is `5596d748764108112bb4428cc97c5c66d4f137d1418ab67a08fdaf9b7ef9c5d2`; its declared status is `SOURCE_FREEZE_NOT_SELF_APPROVAL`. Before the build, all 51 listed file hashes matched. A final recheck found a concurrent change to `research/implementation/p1c/evidence/copied-artifact-manifest.json`; the five Runner source files bound to this binary still match the candidate freeze. This task ran only read/build/run/format-check commands and did not write repo sources.

The production Runner was built into a fresh external target with ordinary `cargo build --offline --locked -p webcodex-runner` on rustc/cargo 1.94.0, Darwin arm64, without bootstrap or feature overrides. Binary SHA-256: `40b7c376e9fe40c83e4e42b750eabc213eaad183536b6a0315e53145ebb6118e`.

The production fixture passed: Runner and harness exited 0; open and exec both returned `remote_durable_authority_unavailable` with `command_started=false`; registration reported remote persistent shell false; no model SSH marker appeared. The exact accepted-base old route returned two results and produced one real `connect` marker. Its Runner exited 0, while the same no-effect assertion caused harness exit 1 after collection.

`cargo fmt --all --check` on production exited 1 with one diff block in `ssh_macos_control_path_tests.rs`. The command was read-only and I did not restore or reformat the file. The identical check on the existing accepted-base copy exited 0; its file differs from the current frozen source (base SHA `e498ec1adaecec21ef76432b7d48feb5a57174e8f10f181e403136d21c26ce73`, current SHA `e47e8b34558e643138ac7965a83e06d983bea131aef0877087406b26af269cb4`), so this is not demonstrated as a pre-existing base failure. The scoped edition-2021 `rustfmt --check --config skip_children=true` over the five owned Runner files exited 0.

Local fallback remains `SOURCE_DERIVED_ONLY` because this fixture lacks the required local payload/profile-init marker positive control. Real remote, Windows/Linux, restart, whole-process-family, C7, and full P1C gates remain `NOT_RUN`. Prior `resume-20261004` raw evidence was not overwritten.

Raw commands, stdout/stderr, exit codes, binary/source hashes, format outcomes, and evidence checksums are in `C2-NATIVE-FINAL-SOURCE-20261004.json`.
