use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const BASE_SHA: &str = "f58e65c6d95bbd91165e97b4a98de694f97ae872";
const BASE_INVENTORY_SHA256: &str =
    "d5d9956f78e8c724e2f1136a9071ba337bb52f031f88de7fd92b5150cc8ee1ee";
const CANDIDATE_STATUS: &str = "UNREVIEWED_CANDIDATE";
const ACCEPTED_STATUS: &str = "SOL_REVIEWED_C2_OVERLAY_ACCEPTED";
const BINARY_TARGET: &str = "webcodex-runner:webcodex-runner";

const REPLACEMENT_PATHS: [&str; 5] = [
    "crates/webcodex-runner/src/main.rs",
    "crates/webcodex-runner/src/webcodex_runner/mod.rs",
    "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
    "crates/webcodex-runner/src/webcodex_runner/ssh.rs",
    "crates/webcodex-runner/src/webcodex_runner/transport.rs",
];

const FROZEN_REPLACEMENT_HASHES: [(&str, &str); 5] = [
    (
        "crates/webcodex-runner/src/main.rs",
        "1444b2012f7eaa311dd8d670b1b3e61556e3da7d0a0331da9b4d66359f7781a5",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/mod.rs",
        "2459ac8aff3265145f092aa769d76df2792eecc12047b5cb565bbd6d74c30d26",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
        "a50183e0a5fe249aa968cbb0c4a095d887aff37daa7d5c65c8fb8305e34e98ff",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/ssh.rs",
        "8257a75c1257f76c8f2afc956b3b00e6b4d2afeda0f0e53b9ce05b172828beaa",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/transport.rs",
        "4967903f60727362bef9a7cbe855d4bede6f920b47d74d6248bfd6d2cc2477d2",
    ),
];

const SIGNED_RAW_ROWS: [(&str, &str, &str, &str); 10] = [
    (
        "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
        "crate::webcodex_runner::persistent_shell::PersistentShellManager::exec_ssh",
        "method::exec",
        "741facf8ddd102eaec3973d69e1a466e69c8d374cbcf7368f80bb7f6ecc67384",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
        "crate::webcodex_runner::persistent_shell::PersistentShellManager::exec_ssh",
        "method::status",
        "c5cec70238ea3eaa479d7f8c864b455c678b8fa3aa10f38a1c79daced48a181b",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/remote_shell.rs",
        "crate::webcodex_runner::remote_shell::RemoteShellTransport::spawn",
        "ManagedChild::spawn",
        "84997c4f43be7f54914ca532868d02141f3d6159f202f1d9f3aedeef04b52176",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/remote_shell.rs",
        "crate::webcodex_runner::remote_shell::RemoteShellTransport::spawn",
        "method::spawn",
        "29dfa0854b35f14af2aba4208dbdb1521f2215d254841c3994c72e1e96f9f24f",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/remote_shell.rs",
        "crate::webcodex_runner::remote_shell::spawn_stderr_reader",
        "method::spawn",
        "21c4a352337774e3591c74c228eeab401282eb336898232131661ea2fecd2681",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/remote_shell.rs",
        "crate::webcodex_runner::remote_shell::spawn_stdout_reader",
        "method::spawn",
        "59c0136c80b1b88ced40bd222cc734c52ae53f0a5c79ec93be30e8f2fd311ad0",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/ssh.rs",
        "crate::webcodex_runner::ssh::SshConnectionPool::persistent_shell_available",
        "Command::new",
        "c2e151b1ddeba80f8c5b4d727c66b8a8b85275eb513f500660d9df05e8c66648",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/ssh.rs",
        "crate::webcodex_runner::ssh::SshConnectionPool::persistent_shell_available",
        "method::status",
        "abf85c22a2bffc19641b3c88ed74eee132830fef8a359e72b730acef02e94fde",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/ssh.rs",
        "crate::webcodex_runner::ssh::SshConnectionPool::prepare_persistent_shell_command",
        "Command::new",
        "424a0cc775918c60a6367e7d2422233d8cc595e485fb8a29c3167895bb705d6c",
    ),
    (
        "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
        "crate::webcodex_runner::persistent_shell::summary_error_result_from_status",
        "method::status",
        "a1ef23d5e4f52f7c18c4aa6de992f80b8618e7af8e987289b6050ae11ffd7242",
    ),
];

pub struct OverlayApplication {
    pub inventory: Value,
    pub status: String,
}

fn exact_keys(value: &Value, expected: &[&str], context: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{context} keys differ: actual={actual:?} expected={expected:?}"
        ))
    }
}

fn string<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("{context}.{field} must be a string"))
}

fn digest_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn targets_are_binary(value: &Value, context: &str) -> Result<(), String> {
    if value["targets"]
        .as_array()
        .is_some_and(|rows| rows.len() == 1 && rows[0].as_str() == Some(BINARY_TARGET))
    {
        Ok(())
    } else {
        Err(format!(
            "{context}.targets must be exactly [{BINARY_TARGET:?}]"
        ))
    }
}

fn base_origin<'a>(inventory: &'a Value, path: &str) -> Result<&'a Value, String> {
    let rows = inventory["production_origin_sources"]
        .as_array()
        .ok_or_else(|| "base production_origin_sources must be an array".to_owned())?;
    let found: Vec<_> = rows
        .iter()
        .filter(|row| row["path"].as_str() == Some(path))
        .collect();
    if found.len() == 1 {
        Ok(found[0])
    } else {
        Err(format!("base origin {path} must occur exactly once"))
    }
}

fn signed_raw_index(row: &Value) -> Result<usize, String> {
    exact_keys(row, &["section", "key", "base_row_sha256"], "raw removal")?;
    if string(row, "section", "raw removal")? != "raw_sites" {
        return Err("raw removal section must be raw_sites".to_owned());
    }
    let key = &row["key"];
    exact_keys(
        key,
        &["file", "symbol", "primitive", "count", "targets"],
        "raw removal key",
    )?;
    let file = string(key, "file", "raw removal key")?;
    let symbol = string(key, "symbol", "raw removal key")?;
    let primitive = string(key, "primitive", "raw removal key")?;
    if key["count"].as_u64() != Some(1) {
        return Err("signed raw removal count must be exactly one".to_owned());
    }
    targets_are_binary(key, "raw removal key")?;
    let digest = string(row, "base_row_sha256", "raw removal")?;
    SIGNED_RAW_ROWS
        .iter()
        .position(
            |(expected_file, expected_symbol, expected_primitive, expected_digest)| {
                file == *expected_file
                    && symbol == *expected_symbol
                    && primitive == *expected_primitive
                    && digest == *expected_digest
            },
        )
        .ok_or_else(|| "raw removal is outside the ten signed exact keys".to_owned())
}

fn validate_overlay(base: &Value, base_bytes: &str, overlay: &Value) -> Result<String, String> {
    exact_keys(
        overlay,
        &[
            "schema_version",
            "status",
            "base_sha",
            "base_inventory_sha256",
            "source_replacements",
            "removed_origin",
            "raw_removals",
            "reference_removals",
        ],
        "overlay",
    )?;
    if overlay["schema_version"].as_u64() != Some(1) {
        return Err("unsupported overlay schema_version".to_owned());
    }
    let status = string(overlay, "status", "overlay")?;
    if status != CANDIDATE_STATUS && status != ACCEPTED_STATUS {
        return Err(
            "overlay status is neither UNREVIEWED_CANDIDATE nor reviewed acceptance".to_owned(),
        );
    }
    if string(overlay, "base_sha", "overlay")? != BASE_SHA {
        return Err("overlay base_sha mismatch".to_owned());
    }
    let actual_base_hash = format!("{:x}", Sha256::digest(base_bytes.as_bytes()));
    if actual_base_hash != BASE_INVENTORY_SHA256
        || string(overlay, "base_inventory_sha256", "overlay")? != BASE_INVENTORY_SHA256
    {
        return Err("immutable P1B inventory SHA-256 mismatch".to_owned());
    }

    let replacements = overlay["source_replacements"]
        .as_array()
        .ok_or_else(|| "source_replacements must be an array".to_owned())?;
    if replacements.len() != REPLACEMENT_PATHS.len() {
        return Err("source_replacements must contain exactly five rows".to_owned());
    }
    let mut seen_paths = BTreeSet::new();
    for row in replacements {
        exact_keys(
            row,
            &["path", "base_sha256", "expected_sha256", "targets"],
            "source replacement",
        )?;
        let path = string(row, "path", "source replacement")?;
        if !REPLACEMENT_PATHS.contains(&path) || !seen_paths.insert(path) {
            return Err(format!(
                "unknown or duplicate source replacement path {path}"
            ));
        }
        targets_are_binary(row, "source replacement")?;
        let baseline = base_origin(base, path)?;
        if string(row, "base_sha256", "source replacement")?
            != string(baseline, "sha256", "base origin")?
            || baseline["targets"] != row["targets"]
        {
            return Err(format!("source replacement old origin mismatch for {path}"));
        }
        let expected_hash = string(row, "expected_sha256", "source replacement")?;
        let frozen_hash = FROZEN_REPLACEMENT_HASHES
            .iter()
            .find(|(frozen_path, _)| *frozen_path == path)
            .map(|(_, hash)| *hash)
            .ok_or_else(|| format!("missing frozen source binding for {path}"))?;
        if !digest_is_valid(expected_hash) || expected_hash != frozen_hash {
            return Err(format!(
                "source replacement is outside the frozen source binding for {path}"
            ));
        }
    }
    let expected_paths: BTreeSet<_> = REPLACEMENT_PATHS.into_iter().collect();
    if seen_paths != expected_paths {
        return Err("source replacement path set is incomplete".to_owned());
    }

    let removed = &overlay["removed_origin"];
    exact_keys(removed, &["path", "sha256", "targets"], "removed_origin")?;
    let removed_path = "crates/webcodex-runner/src/webcodex_runner/remote_shell.rs";
    targets_are_binary(removed, "removed_origin")?;
    let baseline_removed = base_origin(base, removed_path)?;
    if string(removed, "path", "removed_origin")? != removed_path
        || string(removed, "sha256", "removed_origin")?
            != "29a06f8a347cd25e181d9a6a4218b685a01b6c18cde1993fc2c180c3117804b3"
        || removed["sha256"] != baseline_removed["sha256"]
        || removed["targets"] != baseline_removed["targets"]
    {
        return Err("removed_origin differs from the single signed C2 origin".to_owned());
    }

    let raw = overlay["raw_removals"]
        .as_array()
        .ok_or_else(|| "raw_removals must be an array".to_owned())?;
    if raw.len() != SIGNED_RAW_ROWS.len() {
        return Err("raw_removals must contain exactly ten rows".to_owned());
    }
    let mut signed_indexes = BTreeSet::new();
    let base_raw = base["raw_sites"]
        .as_array()
        .ok_or_else(|| "base raw_sites must be an array".to_owned())?;
    for row in raw {
        let index = signed_raw_index(row)?;
        if !signed_indexes.insert(index) {
            return Err("duplicate signed raw removal".to_owned());
        }
        let key = &row["key"];
        let matches: Vec<_> = base_raw
            .iter()
            .filter(|base_row| {
                base_row["file"] == key["file"]
                    && base_row["symbol"] == key["symbol"]
                    && base_row["primitive"] == key["primitive"]
                    && base_row["count"] == key["count"]
                    && base_row["targets"] == key["targets"]
            })
            .collect();
        if matches.len() != 1 {
            return Err(format!("signed raw row must exist exactly once: {key}"));
        }
    }
    if signed_indexes.len() != SIGNED_RAW_ROWS.len() {
        return Err("signed raw removal set is incomplete".to_owned());
    }
    if overlay["reference_removals"]
        .as_array()
        .is_none_or(|rows| !rows.is_empty())
    {
        return Err("reference_removals must be the signed empty array".to_owned());
    }
    Ok(status.to_owned())
}

pub fn apply_sparse_overlay(
    base_bytes: &str,
    overlay_bytes: &str,
) -> Result<OverlayApplication, String> {
    let mut inventory: Value =
        serde_json::from_str(base_bytes).map_err(|error| format!("P1B inventory JSON: {error}"))?;
    let overlay: Value =
        serde_json::from_str(overlay_bytes).map_err(|error| format!("C2 overlay JSON: {error}"))?;
    let status = validate_overlay(&inventory, base_bytes, &overlay)?;

    let removals = overlay["raw_removals"]
        .as_array()
        .expect("validated raw_removals array");
    let raw = inventory["raw_sites"]
        .as_array_mut()
        .expect("validated base raw_sites array");
    for removal in removals {
        let key = &removal["key"];
        let before = raw.len();
        raw.retain(|row| {
            !(row["file"] == key["file"]
                && row["symbol"] == key["symbol"]
                && row["primitive"] == key["primitive"]
                && row["count"] == key["count"]
                && row["targets"] == key["targets"])
        });
        if before != raw.len() + 1 {
            return Err("raw removal was not unique during overlay application".to_owned());
        }
    }

    let origins = inventory["production_origin_sources"]
        .as_array_mut()
        .expect("validated production origins array");
    origins.retain(|row| row["path"] != overlay["removed_origin"]["path"]);
    for replacement in overlay["source_replacements"]
        .as_array()
        .expect("validated source replacements array")
    {
        if let Some(expected_hash) = replacement["expected_sha256"].as_str() {
            let matches: Vec<_> = origins
                .iter_mut()
                .filter(|row| row["path"] == replacement["path"])
                .collect();
            if matches.len() != 1 {
                return Err("source replacement origin was not unique during apply".to_owned());
            }
            matches.into_iter().next().unwrap()["sha256"] = json!(expected_hash);
        }
    }
    Ok(OverlayApplication { inventory, status })
}

fn candidate_bytes() -> (&'static str, &'static str) {
    (
        include_str!("../../../research/implementation/p1b/launch-inventory.json"),
        include_str!("../../../research/implementation/p1c/c2-guard-overlay.json"),
    )
}

#[test]
fn p1c_c2_candidate_overlay_is_sparse_and_fully_bound_to_signed_rows() {
    let (base, overlay) = candidate_bytes();
    let base_value: Value = serde_json::from_str(base).unwrap();
    let result = apply_sparse_overlay(base, overlay).unwrap();
    assert!(matches!(
        result.status.as_str(),
        CANDIDATE_STATUS | ACCEPTED_STATUS
    ));
    for field in [
        "rust_targets",
        "references",
        "logical_surfaces",
        "counts",
        "body_fingerprints",
        "nonrust_assets",
        "nonrust_launch_sites",
        "release_entry_obligations",
        "metric_definitions",
    ] {
        assert_eq!(
            result.inventory[field], base_value[field],
            "overlay changed {field}"
        );
    }
    assert_eq!(
        result.inventory["raw_sites"].as_array().unwrap().len(),
        base_value["raw_sites"].as_array().unwrap().len() - 10
    );
    assert_eq!(
        result.inventory["production_origin_sources"]
            .as_array()
            .unwrap()
            .len(),
        base_value["production_origin_sources"]
            .as_array()
            .unwrap()
            .len()
            - 1
    );
}

#[test]
fn p1c_c2_overlay_rejects_unknown_missing_duplicate_and_unbound_inputs() {
    let (base, overlay) = candidate_bytes();
    let original: Value = serde_json::from_str(overlay).unwrap();
    let rejects = |candidate: Value| {
        assert!(apply_sparse_overlay(base, &candidate.to_string()).is_err());
    };

    let mut unknown = original.clone();
    unknown["automatic_accept_all_origins"] = json!(true);
    rejects(unknown);

    let mut missing = original.clone();
    missing["source_replacements"].as_array_mut().unwrap().pop();
    rejects(missing);

    let mut duplicate = original.clone();
    let first = duplicate["source_replacements"][0].clone();
    duplicate["source_replacements"]
        .as_array_mut()
        .unwrap()
        .push(first);
    rejects(duplicate);

    let mut raw_duplicate = original.clone();
    let first = raw_duplicate["raw_removals"][0].clone();
    raw_duplicate["raw_removals"].as_array_mut().unwrap()[1] = first;
    rejects(raw_duplicate);

    let mut raw_unknown = original.clone();
    raw_unknown["raw_removals"][0]["key"]["symbol"] = json!("crate::other::launch");
    rejects(raw_unknown);

    let mut old_hash = original.clone();
    old_hash["source_replacements"][0]["base_sha256"] = json!("0".repeat(64));
    rejects(old_hash);

    let mut target = original.clone();
    target["removed_origin"]["targets"] = json!(["webcodex-runner:other"]);
    rejects(target);

    let mut missing_origin = original.clone();
    missing_origin["removed_origin"]["path"] = json!("crates/webcodex-runner/src/not_present.rs");
    rejects(missing_origin);

    let mut base = original.clone();
    base["base_sha"] = json!("0000000000000000000000000000000000000000");
    rejects(base);

    let mut inventory_hash = original.clone();
    inventory_hash["base_inventory_sha256"] = json!("0".repeat(64));
    rejects(inventory_hash);

    let mut unknown_status = original.clone();
    unknown_status["status"] = json!("SOL_APPROVED");
    rejects(unknown_status);

    let mut references = original;
    references["reference_removals"] = json!([{"reference":"unapproved"}]);
    rejects(references);
}

#[test]
fn p1c_c2_accepted_status_requires_all_frozen_source_hashes() {
    let (base, overlay) = candidate_bytes();
    let mut accepted: Value = serde_json::from_str(overlay).unwrap();
    accepted["status"] = json!(ACCEPTED_STATUS);
    let application = apply_sparse_overlay(base, &accepted.to_string()).unwrap();
    assert_eq!(application.status, ACCEPTED_STATUS);

    accepted["source_replacements"][0]["expected_sha256"] = Value::Null;
    assert!(apply_sparse_overlay(base, &accepted.to_string()).is_err());
}
