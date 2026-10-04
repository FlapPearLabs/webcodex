use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
const STATUS_CANDIDATE: &str = "UNREVIEWED_CANDIDATE";
const STATUS_ACCEPTED: &str = "SOL_REVIEWED_CHATGPT_SAFE_PROFILE";
const PACKAGE: &str = "webcodex-chatgpt-safe";
const TARGET: &str = "webcodex-chatgpt-safe:webcodex-chatgpt-safe";
const SOURCE: &str = "crates/webcodex-chatgpt-safe/src/main.rs";
const TOOLS: [&str; 8] = [
    "project_list",
    "project_select",
    "files_search",
    "files_read",
    "files_apply_patch",
    "shell_run",
    "git_status",
    "git_diff",
];
pub struct Application {
    pub status: String,
}
fn exact_keys(value: &Value, expected: &[&str], label: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} keys differ: {actual:?}"))
    }
}
fn rows<'a>(inventory: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>, String> {
    inventory[key]
        .as_array_mut()
        .ok_or_else(|| format!("base inventory {key} must be an array"))
}
pub fn apply(inventory: &mut Value, overlay_bytes: &str) -> Result<Application, String> {
    let overlay: Value = serde_json::from_str(overlay_bytes).map_err(|error| error.to_string())?;
    exact_keys(
        &overlay,
        &[
            "schema_version",
            "status",
            "rust_target",
            "production_origin_sources",
            "raw_sites",
            "nonrust_assets",
            "tool_allowlist",
            "profile_facts",
        ],
        "profile overlay",
    )?;
    if overlay["schema_version"].as_u64() != Some(1) {
        return Err("unsupported profile schema_version".into());
    }
    let status = overlay["status"]
        .as_str()
        .ok_or("profile status must be a string")?;
    if !matches!(status, STATUS_CANDIDATE | STATUS_ACCEPTED) {
        return Err("unrecognized profile status".into());
    }

    let target = &overlay["rust_target"];
    exact_keys(target, &["package", "target", "file"], "rust_target")?;
    if target
        != &json!({"package":PACKAGE,"target":PACKAGE,"file":"crates/webcodex-chatgpt-safe/src/main.rs"})
    {
        return Err("profile target is outside the exact ChatGPT-safe binary".into());
    }
    let origins = overlay["production_origin_sources"]
        .as_array()
        .ok_or("profile origins must be an array")?;
    if origins.len() != 1 {
        return Err("profile must bind exactly its main source origin".into());
    }
    let origin = &origins[0];
    exact_keys(origin, &["path", "sha256", "targets"], "profile origin")?;
    let digest = origin["sha256"]
        .as_str()
        .ok_or("profile origin sha256 must be a string")?;
    if origin["path"] != SOURCE
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || origin["targets"] != json!([TARGET])
    {
        return Err(
            "profile source origin must bind exact path, lowercase SHA-256, and target".into(),
        );
    }
    let raw = overlay["raw_sites"]
        .as_array()
        .ok_or("profile raw_sites must be an array")?;
    let expected = [("crate::spawn_broker", 1)];
    if raw.len() != expected.len() {
        return Err("profile must bind exactly one brokered launch site".into());
    }
    let mut seen = BTreeSet::new();
    for row in raw {
        exact_keys(
            row,
            &["file", "symbol", "primitive", "count", "targets"],
            "profile raw site",
        )?;
        let symbol = row["symbol"]
            .as_str()
            .ok_or("raw-site symbol must be a string")?;
        if row["file"] != SOURCE
            || row["primitive"] != "method::spawn_with_toolchain"
            || row["count"] != 1
            || row["targets"] != json!([TARGET])
            || !expected.contains(&(symbol, 1))
            || !seen.insert(symbol.to_owned())
        {
            return Err("profile raw site differs from the one exact brokered launch".into());
        }
    }
    if overlay["nonrust_assets"] != json!([]) {
        return Err("profile has no separate non-Rust release assets".into());
    }
    let tools = overlay["tool_allowlist"]
        .as_array()
        .ok_or("tool_allowlist must be an array")?;
    let tools: Vec<_> = tools
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or("tool allowlist entries must be strings")
        })
        .collect::<Result<_, _>>()?;
    if tools.as_slice() != TOOLS.as_slice() {
        return Err("profile tool allowlist differs from the exact source allowlist".into());
    }
    if overlay["profile_facts"]
        != json!([{
            "class":"A",
            "caller":"crate::shell",
            "fact":"shell_run is a Class A logical caller routed through the shared spawn_broker",
            "source":SOURCE
        }, {
            "class":"A",
            "caller":"crate::spawn_python",
            "fact":"the Python file helper is a Class A logical caller routed through the shared spawn_broker",
            "source":SOURCE
        }])
    {
        return Err("profile launch fact differs from its fixed classification claim".into());
    }

    let rust_targets = rows(inventory, "rust_targets")?;
    if rust_targets
        .iter()
        .any(|row| row["package"] == PACKAGE || row["target"] == PACKAGE)
    {
        return Err("ChatGPT-safe target already exists in base inventory".into());
    }
    rust_targets.push(target.clone());
    rows(inventory, "production_origin_sources")?.push(origin.clone());
    rows(inventory, "raw_sites")?.extend(raw.iter().cloned());
    if !overlay["nonrust_assets"].as_array().unwrap().is_empty() {
        rows(inventory, "nonrust_assets")?.extend(
            overlay["nonrust_assets"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
    }
    Ok(Application {
        status: status.to_owned(),
    })
}

pub fn assert_source_allowlist(root: &Path, overlay_bytes: &str) -> Result<(), String> {
    let source = fs::read_to_string(root.join(SOURCE)).map_err(|error| error.to_string())?;
    let syntax = syn::parse_file(&source).map_err(|error| error.to_string())?;
    let actual = syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == "SAFE_TOOLS" => match &*item.expr {
                syn::Expr::Array(array) => Some(
                    array
                        .elems
                        .iter()
                        .map(|expr| match expr {
                            syn::Expr::Lit(lit) => match &lit.lit {
                                syn::Lit::Str(value) => Some(value.value()),
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()?,
                ),
                _ => None,
            },
            _ => None,
        })
        .ok_or("source SAFE_TOOLS array is missing or non-literal")?;
    let overlay: Value = serde_json::from_str(overlay_bytes).map_err(|error| error.to_string())?;
    let origin_digest = overlay["production_origin_sources"][0]["sha256"]
        .as_str()
        .ok_or("profile origin SHA-256 is missing")?;
    let actual_digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    if origin_digest != actual_digest {
        return Err("profile origin SHA-256 differs from source bytes".into());
    }
    let expected: Vec<_> = overlay["tool_allowlist"]
        .as_array()
        .ok_or("tool_allowlist must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or("tool allowlist entries must be strings")
        })
        .collect::<Result<_, _>>()?;
    if actual == expected {
        Ok(())
    } else {
        Err("source SAFE_TOOLS differs from profile overlay".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay() -> Value {
        serde_json::from_str(include_str!(
            "../../../research/implementation/dogfood/profile-launch-overlay.json"
        ))
        .unwrap()
    }

    #[test]
    fn profile_source_mutation_rejects_unregistered_launcher() {
        let repository_source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../webcodex-chatgpt-safe/src/main.rs");
        let source = fs::read_to_string(repository_source).unwrap();
        syn::parse_file(&source).expect("current production source is valid Rust");

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source_path = root.join(SOURCE);
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::write(&source_path, &source).unwrap();

        let mut pinned = overlay();
        pinned["production_origin_sources"][0]["sha256"] =
            json!(format!("{:x}", Sha256::digest(source.as_bytes())));
        let pinned_bytes = pinned.to_string();
        assert_source_allowlist(root, &pinned_bytes).unwrap();

        let launcher_mutation = format!(
            "{source}\nfn unregistered_launcher() {{ let _ = std::process::Command::new(\"/bin/sh\").spawn(); }}\n"
        );
        syn::parse_file(&launcher_mutation).expect("launcher mutation is valid Rust syntax");
        fs::write(&source_path, &launcher_mutation).unwrap();
        assert!(assert_source_allowlist(root, &pinned_bytes).is_err());

        let allowlist_mutation = source.replacen(
            "    \"git_diff\",\n",
            "    \"git_diff\",\n    \"newtool\",\n",
            1,
        );
        assert_ne!(
            allowlist_mutation, source,
            "source allowlist literal must be found"
        );
        syn::parse_file(&allowlist_mutation).expect("allowlist mutation is valid Rust syntax");
        fs::write(&source_path, &allowlist_mutation).unwrap();
        let mut mutated_pin = pinned;
        mutated_pin["production_origin_sources"][0]["sha256"] = json!(format!(
            "{:x}",
            Sha256::digest(allowlist_mutation.as_bytes())
        ));
        assert!(assert_source_allowlist(root, &mutated_pin.to_string()).is_err());
    }

    #[test]
    fn profile_overlay_is_additive_and_rejects_unaccepted_launches() {
        let mut base = json!({"rust_targets":[],"production_origin_sources":[],"raw_sites":[],"nonrust_assets":[],"status":"unchanged","references":[{"sentinel":1}],"counts":{"sentinel":1},"body_fingerprints":[{"sentinel":1}],"metric_definitions":[{"sentinel":1}]});
        let before = base.clone();
        let application = apply(&mut base, &overlay().to_string()).unwrap();
        assert!(matches!(
            application.status.as_str(),
            STATUS_CANDIDATE | STATUS_ACCEPTED
        ));
        for key in [
            "rust_targets",
            "production_origin_sources",
            "raw_sites",
            "nonrust_assets",
        ] {
            assert_eq!(
                base[key].as_array().unwrap().len(),
                overlay()[key].as_array().map_or(1, |rows| rows.len())
            );
        }
        for key in [
            "status",
            "references",
            "counts",
            "body_fingerprints",
            "metric_definitions",
        ] {
            assert_eq!(
                base[key], before[key],
                "profile overlay must preserve base {key}"
            );
        }

        for mutate in [0, 1, 2, 3, 4, 5] {
            let mut changed = overlay();
            match mutate {
                0 => {
                    let duplicate = changed["raw_sites"][0].clone();
                    changed["raw_sites"].as_array_mut().unwrap().push(duplicate);
                }
                1 => {
                    changed["production_origin_sources"][0]["path"] =
                        json!("crates/other/src/lib.rs")
                }
                2 => changed["profile_facts"][0]["class"] = json!("B"),
                3 => changed["nonrust_assets"] = json!([{"path":"unaccepted"}]),
                4 => changed["unreviewed_extension"] = json!(true),
                _ => {
                    changed.as_object_mut().unwrap().remove("profile_facts");
                }
            }
            let mut candidate = before.clone();
            assert!(apply(&mut candidate, &changed.to_string()).is_err());
        }
    }
}
