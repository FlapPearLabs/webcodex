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
/// Operator-only service module. It is reachable from the `webcodex chatgpt`
/// CLI control plane and never from the MCP tool surface, so its `launchctl`
/// site is an operator/host fixed control-plane launch, not model authority.
const SERVICE_SOURCE: &str = "crates/webcodex-chatgpt-safe/src/service.rs";
const SERVICE_SYMBOL: &str = "crate::service::launchctl";
/// The only executable the operator service layer may launch, as a literal.
/// Nothing may be parameterized into this position.
const SERVICE_EXECUTABLE: &str = "/bin/launchctl";
const TOOLS: [&str; 16] = [
    "project_list",
    "project_select",
    "project_current",
    "files_search",
    "files_read",
    "files_apply_patch",
    "shell_run",
    "job_start",
    "job_poll",
    "job_cancel",
    "git_status",
    "git_diff",
    "lsp_symbols",
    "lsp_definition",
    "lsp_references",
    "lsp_diagnostics",
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

/// Collect the first argument of every `<path>::Command::new(..)` call in a
/// parsed file.
///
/// Only plain string literals are accepted. A non-literal program (a variable,
/// a `format!`, a concatenation) is an error rather than a silently skipped
/// row: an operator-only launch surface is only acceptable when its executable
/// is pinned to a literal that review can read, so an unreadable program must
/// fail closed instead of being waved through as "still just a service helper".
fn command_new_programs(syntax: &syn::File) -> Result<Vec<String>, String> {
    struct Visitor<'a> {
        literals: Vec<String>,
        parameterized: usize,
        marker: std::marker::PhantomData<&'a ()>,
    }
    impl<'a> syn::visit::Visit<'a> for Visitor<'a> {
        fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
            let path = match &*call.func {
                syn::Expr::Path(path) => &path.path,
                _ => {
                    syn::visit::visit_expr_call(self, call);
                    return;
                }
            };
            let segments: Vec<_> = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if segments.len() >= 2
                && segments[segments.len() - 1] == "new"
                && segments[segments.len() - 2] == "Command"
            {
                match call.args.first() {
                    Some(syn::Expr::Lit(literal)) => match &literal.lit {
                        syn::Lit::Str(value) => {
                            self.literals.push(value.value());
                            return;
                        }
                        _ => self.parameterized += 1,
                    },
                    _ => self.parameterized += 1,
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let mut visitor = Visitor {
        literals: Vec::new(),
        parameterized: 0,
        marker: std::marker::PhantomData,
    };
    syn::visit::visit_file(&mut visitor, syntax);
    if visitor.parameterized > 0 {
        return Err(format!(
            "operator service source has {} non-literal Command::new program(s); \
             operator launches must be pinned to reviewable literals",
            visitor.parameterized
        ));
    }
    Ok(visitor.literals)
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
    if origins.len() != 2 {
        return Err("profile must bind exactly its main and service source origins".into());
    }
    let expected_origins = [(SOURCE, TARGET), (SERVICE_SOURCE, TARGET)];
    let mut seen_origins = BTreeSet::new();
    for origin in origins {
        exact_keys(origin, &["path", "sha256", "targets"], "profile origin")?;
        let digest = origin["sha256"]
            .as_str()
            .ok_or("profile origin sha256 must be a string")?;
        let path = origin["path"]
            .as_str()
            .ok_or("profile origin path must be a string")?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || origin["targets"] != json!([TARGET])
            || !expected_origins
                .iter()
                .any(|(expected_path, expected_target)| {
                    path == *expected_path && origin["targets"] == json!([*expected_target])
                })
            || !seen_origins.insert(path.to_owned())
        {
            return Err(
                "profile source origin must bind exact path, lowercase SHA-256, and target".into(),
            );
        }
    }
    if !origins
        .iter()
        .any(|origin| origin["path"] == SERVICE_SOURCE)
    {
        return Err("profile must bind its operator service source origin".into());
    }
    let raw = overlay["raw_sites"]
        .as_array()
        .ok_or("profile raw_sites must be an array")?;
    // Exactly one brokered launch site, plus the two primitives that make up
    // the single operator-only launchd call site (the Command::new and its
    // .output()). No third production launch primitive is acceptable.
    if raw.len() != 3 {
        return Err(
            "profile must bind exactly one brokered launch site and one operator-only service site"
                .into(),
        );
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
        let file = row["file"].as_str().unwrap_or_default();
        let primitive = row["primitive"].as_str().unwrap_or_default();
        let is_broker = file == SOURCE
            && symbol == "crate::spawn_broker"
            && primitive == "method::spawn_with_toolchain"
            && row["count"] == 1;
        let is_service = file == SERVICE_SOURCE
            && symbol == SERVICE_SYMBOL
            && matches!(primitive, "Command::new" | "method::output")
            && row["count"] == 1;
        if row["targets"] != json!([TARGET])
            || (!is_broker && !is_service)
            || !seen.insert((file.to_owned(), symbol.to_owned(), primitive.to_owned()))
        {
            return Err("profile raw site differs from the one exact brokered launch".into());
        }
    }
    if !seen.contains(&(
        SOURCE.to_owned(),
        "crate::spawn_broker".to_owned(),
        "method::spawn_with_toolchain".to_owned(),
    )) {
        return Err("profile must bind its brokered launch site".into());
    }
    if seen.len() != 3 {
        return Err("profile raw sites must cover the broker and the service launcher".into());
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
        }, {
            "class":"A",
            "caller":"crate::JobRegistry::spawn_job",
            "fact":"job_start is a Class A logical caller that reuses prepare_shell and the shared spawn_broker; there is no alternate naked Command/spawn path",
            "source":SOURCE
        }, {
            "class":"A",
            "caller":"webcodex_lsp::LspSupervisor",
            "fact":"LSP server launch is brokered inside webcodex-lsp via ExecutionBroker::spawn_with_toolchain; the MCP layer never supplies an executable, args, or initialization command",
            "source":SOURCE
        }, {
            "class":"D",
            "caller":SERVICE_SYMBOL,
            "fact":"the operator-only launchd control plane runs launchctl as a fixed literal; it is unreachable from the MCP tool surface, takes no model-supplied executable, and never carries model execution authority",
            "source":SERVICE_SOURCE
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
    rows(inventory, "production_origin_sources")?.extend(origins.iter().cloned());
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
    let service =
        fs::read_to_string(root.join(SERVICE_SOURCE)).map_err(|error| error.to_string())?;
    // The operator service layer may only ever launch the fixed launchd
    // literal. This is bound to the Command::new call sites structurally so
    // that introducing any other executable, or parameterizing this one, fails
    // closed instead of being waved through as "still just a service helper".
    let service_syntax = syn::parse_file(&service).map_err(|error| error.to_string())?;
    let programs = command_new_programs(&service_syntax)?;
    if programs.is_empty() {
        return Err("operator service source exposes no Command::new launch site".into());
    }
    for program in &programs {
        if *program != SERVICE_EXECUTABLE {
            return Err(format!(
                "operator service source may only launch the fixed {SERVICE_EXECUTABLE}, found {program}"
            ));
        }
    }
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
    let origins = overlay["production_origin_sources"]
        .as_array()
        .ok_or("profile origin SHA-256 is missing")?;
    for (path, bytes) in [
        (SOURCE, source.as_bytes()),
        (SERVICE_SOURCE, service.as_bytes()),
    ] {
        let bound = origins
            .iter()
            .find(|row| row["path"] == path)
            .ok_or_else(|| format!("profile does not bind an origin for {path}"))?;
        let pinned = bound["sha256"]
            .as_str()
            .ok_or("profile origin SHA-256 must be a string")?;
        let actual_digest = format!("{:x}", Sha256::digest(bytes));
        if pinned != actual_digest {
            return Err(format!(
                "profile origin SHA-256 differs from source bytes: {path}"
            ));
        }
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

    /// Repository root, derived from this test crate's manifest directory
    /// (`<repo>/crates/webcodex-process`).
    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("test crate must live under <repo>/crates")
            .to_path_buf()
    }

    #[test]
    fn profile_source_mutation_rejects_unregistered_launcher() {
        let root = repository_root();
        let source = fs::read_to_string(root.join(SOURCE)).unwrap();
        let service = fs::read_to_string(root.join(SERVICE_SOURCE)).unwrap();
        syn::parse_file(&source).expect("current production source is valid Rust");
        syn::parse_file(&service).expect("current operator service source is valid Rust");

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source_path = root.join(SOURCE);
        let service_path = root.join(SERVICE_SOURCE);
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::write(&source_path, &source).unwrap();
        fs::write(&service_path, &service).unwrap();

        let mut pinned = overlay();
        for (path, bytes) in [
            (SOURCE, source.as_bytes()),
            (SERVICE_SOURCE, service.as_bytes()),
        ] {
            let row = pinned["production_origin_sources"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["path"] == path)
                .unwrap();
            row["sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
        }
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
        for row in mutated_pin["production_origin_sources"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
        {
            if row["path"] == SOURCE {
                row["sha256"] = json!(format!(
                    "{:x}",
                    Sha256::digest(allowlist_mutation.as_bytes())
                ));
            }
        }
        assert!(assert_source_allowlist(root, &mutated_pin.to_string()).is_err());
    }

    /// The operator service layer is the one production surface allowed to run
    /// a fixed non-brokered executable. These controls pin that exception to
    /// `/bin/launchctl` and prove it fails closed on any other program, on a
    /// parameterized program, and on losing its launch site entirely.
    #[test]
    fn operator_service_launch_is_pinned_to_the_fixed_launchd_literal() {
        let service = fs::read_to_string(repository_root().join(SERVICE_SOURCE)).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join(SOURCE).parent().unwrap()).unwrap();
        fs::write(root.join(SOURCE), "{}").unwrap();
        fs::write(root.join(SERVICE_SOURCE), &service).unwrap();

        // Baseline: the real service source launches exactly one program, and
        // it is the fixed launchd literal.
        let programs = command_new_programs(&syn::parse_file(&service).unwrap()).unwrap();
        assert_eq!(programs, vec![SERVICE_EXECUTABLE.to_owned()]);

        // A foreign executable must be observable in the AST and then rejected
        // by the same gate that accepts the baseline.
        let foreign = service.replacen(
            "Command::new(\"/bin/launchctl\")",
            "Command::new(\"/bin/sh\")",
            1,
        );
        assert_ne!(foreign, service, "service launch literal must be present");
        syn::parse_file(&foreign).expect("foreign launch mutation is valid Rust");
        let programs = command_new_programs(&syn::parse_file(&foreign).unwrap()).unwrap();
        assert!(
            programs.iter().any(|program| program == "/bin/sh"),
            "the foreign executable must be observable before rejection"
        );
        assert!(
            programs.iter().all(|program| program != SERVICE_EXECUTABLE),
            "the fixed literal must be gone in this mutation"
        );
        fs::write(root.join(SERVICE_SOURCE), &foreign).unwrap();
        assert!(assert_source_allowlist(root, &overlay().to_string()).is_err());

        // A non-literal program is unreviewable, so it must fail closed with an
        // error rather than being skipped as "not a literal we recognize".
        let parameterized = format!(
            "{service}\nfn widened(program: &str) {{ let _ = Command::new(program).status(); }}\n"
        );
        syn::parse_file(&parameterized).expect("parameterized mutation is valid Rust");
        assert!(
            command_new_programs(&syn::parse_file(&parameterized).unwrap()).is_err(),
            "a non-literal program must fail closed, not pass silently"
        );
        fs::write(root.join(SERVICE_SOURCE), &parameterized).unwrap();
        assert!(assert_source_allowlist(root, &overlay().to_string()).is_err());
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

        for mutate in [
            "duplicate-raw-site",
            "foreign-origin",
            "downgraded-class",
            "unaccepted-asset",
            "unknown-key",
            "missing-facts",
            "missing-service-origin",
            "service-site-without-fixed-symbol",
            "service-site-with-foreign-file",
            "service-site-with-extra-primitive",
        ] {
            let mut changed = overlay();
            match mutate {
                "duplicate-raw-site" => {
                    let duplicate = changed["raw_sites"][0].clone();
                    changed["raw_sites"].as_array_mut().unwrap().push(duplicate);
                }
                "foreign-origin" => {
                    changed["production_origin_sources"][0]["path"] =
                        json!("crates/other/src/lib.rs")
                }
                "downgraded-class" => changed["profile_facts"][0]["class"] = json!("B"),
                "unaccepted-asset" => changed["nonrust_assets"] = json!([{"path":"unaccepted"}]),
                "unknown-key" => changed["unreviewed_extension"] = json!(true),
                "missing-facts" => {
                    changed.as_object_mut().unwrap().remove("profile_facts");
                }
                "missing-service-origin" => {
                    changed["production_origin_sources"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|row| row["path"] != SERVICE_SOURCE);
                }
                "service-site-without-fixed-symbol" => {
                    for row in changed["raw_sites"].as_array_mut().unwrap().iter_mut() {
                        if row["file"] == SERVICE_SOURCE {
                            row["symbol"] = json!("crate::service::arbitrary");
                        }
                    }
                }
                "service-site-with-foreign-file" => {
                    for row in changed["raw_sites"].as_array_mut().unwrap().iter_mut() {
                        if row["file"] == SERVICE_SOURCE {
                            row["file"] = json!("crates/webcodex-chatgpt-safe/src/main.rs");
                        }
                    }
                }
                _ => {
                    for row in changed["raw_sites"].as_array_mut().unwrap().iter_mut() {
                        if row["file"] == SERVICE_SOURCE && row["primitive"] == "Command::new" {
                            row["primitive"] = json!("method::spawn");
                        }
                    }
                }
            }
            let mut candidate = before.clone();
            assert!(
                apply(&mut candidate, &changed.to_string()).is_err(),
                "overlay mutation {mutate} must be rejected"
            );
        }
    }
}
