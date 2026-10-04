//! Independent production-target subprocess inventory.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, Expr, Item, Meta};

#[path = "p1c_c2_overlay.rs"]
mod p1c_c2_overlay;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Truth {
    True,
    False,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RawSite {
    file: String,
    symbol: String,
    primitive: String,
    count: usize,
    targets: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolReference {
    file: String,
    symbol: String,
    reference: String,
    targets: BTreeSet<String>,
    count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProductionOriginSource {
    sha256: String,
    targets: BTreeSet<String>,
}

#[derive(Default)]
struct Scan {
    sites: BTreeMap<(String, String, String), RawSite>,
    references: BTreeMap<(String, String, String, usize, usize), SymbolReference>,
    site_occurrences: BTreeSet<(String, String, String, usize, usize)>,
    diagnostics: BTreeSet<String>,
    production_origins: BTreeMap<String, ProductionOriginSource>,
}

fn cfg_truth(meta: &Meta) -> Truth {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Truth::False,
        Meta::Path(_) | Meta::NameValue(_) => Truth::Unknown,
        Meta::List(list) => {
            let parsed = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::token::Comma>::parse_terminated,
            );
            let Ok(values) = parsed else {
                return Truth::Unknown;
            };
            let name = list
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string());
            match name.as_deref() {
                Some("all") if values.iter().any(|value| cfg_truth(value) == Truth::False) => {
                    Truth::False
                }
                Some("all") if values.iter().all(|value| cfg_truth(value) == Truth::True) => {
                    Truth::True
                }
                Some("all") => Truth::Unknown,
                Some("any") if values.iter().any(|value| cfg_truth(value) == Truth::True) => {
                    Truth::True
                }
                Some("any") if values.iter().all(|value| cfg_truth(value) == Truth::False) => {
                    Truth::False
                }
                Some("any") => Truth::Unknown,
                Some("not") => values
                    .first()
                    .map(cfg_truth)
                    .map(|value| match value {
                        Truth::True => Truth::False,
                        Truth::False => Truth::True,
                        Truth::Unknown => Truth::Unknown,
                    })
                    .unwrap_or(Truth::Unknown),
                _ => Truth::Unknown,
            }
        }
    }
}

fn attrs_possible(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if attr.path().is_ident("cfg") {
            if attr
                .parse_args::<Meta>()
                .map(|meta| cfg_truth(&meta) == Truth::False)
                .unwrap_or(false)
            {
                return false;
            }
        }
        if attr.path().is_ident("cfg_attr") {
            if let Ok(items) = attr.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::token::Comma>::parse_terminated,
            ) {
                if items
                    .first()
                    .is_some_and(|condition| cfg_truth(condition) == Truth::True)
                    && items.iter().skip(1).any(|item| {
                        item.path().is_ident("cfg")
                            && item
                                .require_list()
                                .map(|list| list.tokens.to_string().replace(' ', "") == "test")
                                .unwrap_or(false)
                    })
                {
                    return false;
                }
            }
        }
    }
    true
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(x) => &x.attrs,
        Item::Enum(x) => &x.attrs,
        Item::ExternCrate(x) => &x.attrs,
        Item::Fn(x) => &x.attrs,
        Item::ForeignMod(x) => &x.attrs,
        Item::Impl(x) => &x.attrs,
        Item::Macro(x) => &x.attrs,
        Item::Mod(x) => &x.attrs,
        Item::Static(x) => &x.attrs,
        Item::Struct(x) => &x.attrs,
        Item::Trait(x) => &x.attrs,
        Item::TraitAlias(x) => &x.attrs,
        Item::Type(x) => &x.attrs,
        Item::Union(x) => &x.attrs,
        Item::Use(x) => &x.attrs,
        _ => &[],
    }
}

fn expr_attrs(expr: &Expr) -> Option<&[Attribute]> {
    Some(match expr {
        Expr::Array(value) => &value.attrs,
        Expr::Assign(value) => &value.attrs,
        Expr::Async(value) => &value.attrs,
        Expr::Await(value) => &value.attrs,
        Expr::Binary(value) => &value.attrs,
        Expr::Block(value) => &value.attrs,
        Expr::Break(value) => &value.attrs,
        Expr::Call(value) => &value.attrs,
        Expr::Cast(value) => &value.attrs,
        Expr::Closure(value) => &value.attrs,
        Expr::Const(value) => &value.attrs,
        Expr::Continue(value) => &value.attrs,
        Expr::Field(value) => &value.attrs,
        Expr::ForLoop(value) => &value.attrs,
        Expr::Group(value) => &value.attrs,
        Expr::If(value) => &value.attrs,
        Expr::Index(value) => &value.attrs,
        Expr::Infer(value) => &value.attrs,
        Expr::Let(value) => &value.attrs,
        Expr::Lit(value) => &value.attrs,
        Expr::Loop(value) => &value.attrs,
        Expr::Macro(value) => &value.attrs,
        Expr::Match(value) => &value.attrs,
        Expr::MethodCall(value) => &value.attrs,
        Expr::Paren(value) => &value.attrs,
        Expr::Path(value) => &value.attrs,
        Expr::Range(value) => &value.attrs,
        Expr::RawAddr(value) => &value.attrs,
        Expr::Reference(value) => &value.attrs,
        Expr::Repeat(value) => &value.attrs,
        Expr::Return(value) => &value.attrs,
        Expr::Struct(value) => &value.attrs,
        Expr::Try(value) => &value.attrs,
        Expr::TryBlock(value) => &value.attrs,
        Expr::Tuple(value) => &value.attrs,
        Expr::Unary(value) => &value.attrs,
        Expr::Unsafe(value) => &value.attrs,
        Expr::Verbatim(_) => &[],
        Expr::While(value) => &value.attrs,
        Expr::Yield(value) => &value.attrs,
        _ => return None,
    })
}

fn aliases_for(items: &[Item]) -> BTreeSet<String> {
    fn walk(tree: &syn::UseTree, path: &mut Vec<String>, aliases: &mut BTreeSet<String>) {
        match tree {
            syn::UseTree::Path(x) => {
                path.push(x.ident.to_string());
                walk(&x.tree, path, aliases);
                path.pop();
            }
            syn::UseTree::Name(x)
                if x.ident == "Command" && path.iter().any(|part| part == "process") =>
            {
                aliases.insert("Command".into());
            }
            syn::UseTree::Rename(x)
                if x.ident == "Command" && path.iter().any(|part| part == "process") =>
            {
                aliases.insert(x.rename.to_string());
            }
            syn::UseTree::Group(x) => {
                for item in &x.items {
                    walk(item, path, aliases);
                }
            }
            _ => {}
        }
    }
    let mut aliases = BTreeSet::new();
    for item in items {
        if attrs_possible(item_attrs(item)) {
            if let Item::Use(import) = item {
                walk(&import.tree, &mut Vec::new(), &mut aliases);
            }
        }
    }
    aliases
}

fn native_aliases_for(items: &[Item]) -> BTreeMap<String, String> {
    const APIS: &[&str] = &[
        "ShellExecuteW",
        "ShellExecuteExW",
        "AuthorizationExecuteWithPrivileges",
    ];
    fn walk(tree: &syn::UseTree, path: &mut Vec<String>, aliases: &mut BTreeMap<String, String>) {
        match tree {
            syn::UseTree::Path(x) => {
                path.push(x.ident.to_string());
                walk(&x.tree, path, aliases);
                path.pop();
            }
            syn::UseTree::Name(x) if APIS.contains(&x.ident.to_string().as_str()) => {
                aliases.insert(x.ident.to_string(), x.ident.to_string());
            }
            syn::UseTree::Rename(x) if APIS.contains(&x.ident.to_string().as_str()) => {
                aliases.insert(x.rename.to_string(), x.ident.to_string());
            }
            syn::UseTree::Group(x) => {
                for item in &x.items {
                    walk(item, path, aliases);
                }
            }
            _ => {}
        }
    }
    let mut aliases = BTreeMap::new();
    for item in items {
        if attrs_possible(item_attrs(item)) {
            if let Item::Use(import) = item {
                walk(&import.tree, &mut Vec::new(), &mut aliases);
            }
        }
    }
    aliases
}

struct Visitor<'a> {
    file: &'a str,
    target: &'a str,
    aliases: BTreeSet<String>,
    native_aliases: BTreeMap<String, String>,
    symbol: String,
    scan: &'a mut Scan,
}

impl Visitor<'_> {
    fn record(&mut self, primitive: &str, span: proc_macro2::Span) {
        let start = span.start();
        let occurrence = (
            self.file.to_owned(),
            self.symbol.clone(),
            primitive.to_owned(),
            start.line,
            start.column,
        );
        if !self.scan.site_occurrences.insert(occurrence) {
            if let Some(row) = self.scan.sites.get_mut(&(
                self.file.to_owned(),
                self.symbol.clone(),
                primitive.to_owned(),
            )) {
                row.targets.insert(self.target.to_owned());
            }
            return;
        }
        let key = (
            self.file.to_owned(),
            self.symbol.clone(),
            primitive.to_owned(),
        );
        let row = self.scan.sites.entry(key).or_insert_with(|| RawSite {
            file: self.file.to_owned(),
            symbol: self.symbol.clone(),
            primitive: primitive.into(),
            count: 0,
            targets: BTreeSet::new(),
        });
        row.count += 1;
        row.targets.insert(self.target.to_owned());
    }

    fn record_reference(&mut self, reference: &str, span: proc_macro2::Span) {
        let start = span.start();
        let key = (
            self.file.to_owned(),
            self.symbol.clone(),
            reference.to_owned(),
            start.line,
            start.column,
        );
        let row = self
            .scan
            .references
            .entry(key)
            .or_insert_with(|| SymbolReference {
                file: self.file.to_owned(),
                symbol: self.symbol.clone(),
                reference: reference.to_owned(),
                targets: BTreeSet::new(),
                count: 0,
            });
        if row.targets.is_empty() {
            row.count = 1;
        }
        row.targets.insert(self.target.to_owned());
    }

    fn macro_has_process_launch(&self, tokens: &str) -> bool {
        let compact = tokens.replace(" :: ", "::").replace(" . ", ".");
        let mut aliases = self.aliases.clone();
        for suffix in compact.split("std::process::Command as ").skip(1) {
            if let Some(alias) = suffix
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .next()
                .filter(|alias| !alias.is_empty())
            {
                aliases.insert(alias.to_owned());
            }
        }
        let constructor = compact.contains("Command::new")
            || aliases
                .iter()
                .any(|alias| compact.contains(&format!("{alias}::new")));
        let terminal = [
            ".spawn",
            ".spawn_with_options",
            ".spawn_with_toolchain",
            ".into_command",
            ".output",
            ".status",
            ".wait_with_output",
            ".exec",
        ]
        .iter()
        .any(|method| compact.contains(method));
        let managed_child = compact.contains("ManagedChild")
            && ["spawn", "spawn_with_options", "spawn_with_toolchain"]
                .iter()
                .any(|method| compact.contains(method));
        let broker = ["ExecutionBroker", "BrokeredGit", "broker"]
            .iter()
            .any(|name| compact.contains(name))
            && ["spawn", "spawn_with_toolchain", "into_command"]
                .iter()
                .any(|method| compact.contains(method));
        (constructor && terminal) || managed_child || broker
    }

    fn macro_has_native_api(&self, tokens: &str) -> bool {
        let compact = tokens.replace(" :: ", "::");
        [
            "ShellExecuteW",
            "ShellExecuteExW",
            "AuthorizationExecuteWithPrivileges",
        ]
        .iter()
        .any(|api| compact.contains(api))
            || self
                .native_aliases
                .keys()
                .any(|alias| compact.contains(alias))
    }
}

fn macro_tokens_without_literals(tokens: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    fn filter(stream: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
        use proc_macro2::TokenTree;
        stream
            .into_iter()
            .filter_map(|token| match token {
                TokenTree::Group(group) => Some(TokenTree::Group(proc_macro2::Group::new(
                    group.delimiter(),
                    filter(group.stream()),
                ))),
                TokenTree::Literal(_) => None,
                other => Some(other),
            })
            .collect()
    }
    filter(tokens)
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
    fn visit_item(&mut self, item: &'ast Item) {
        if attrs_possible(item_attrs(item)) {
            syn::visit::visit_item(self, item);
        }
    }
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        if let syn::Stmt::Macro(item) = stmt {
            if !attrs_possible(&item.attrs) {
                return;
            }
        }
        syn::visit::visit_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr_attrs(expr) {
            Some(attrs) if attrs_possible(attrs) => syn::visit::visit_expr(self, expr),
            Some(_) => {}
            None => {
                self.scan.diagnostics.insert(format!(
                    "{}::{} has unsupported expression attributes",
                    self.file, self.symbol
                ));
            }
        }
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if attrs_possible(&local.attrs) {
            syn::visit::visit_local(self, local);
        }
    }
    fn visit_pat(&mut self, pat: &'ast syn::Pat) {
        if !matches!(pat, syn::Pat::Path(_)) {
            syn::visit::visit_pat(self, pat);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !attrs_possible(&item.attrs) {
            return;
        }
        let qualified = format!("{}::{}", self.symbol, item.sig.ident);
        let previous = std::mem::replace(&mut self.symbol, qualified);
        syn::visit::visit_item_fn(self, item);
        self.symbol = previous;
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let aliases = self.aliases.clone();
        let items = block
            .stmts
            .iter()
            .filter_map(|stmt| match stmt {
                syn::Stmt::Item(item) => Some(item.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        self.aliases.extend(aliases_for(&items));
        let native_aliases = self.native_aliases.clone();
        self.native_aliases.extend(native_aliases_for(&items));
        syn::visit::visit_block(self, block);
        self.aliases = aliases;
        self.native_aliases = native_aliases;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !attrs_possible(&item.attrs) {
            return;
        }
        if self.symbol.ends_with("::PreparedExecutionEnvironment") {
            self.record_reference(
                &format!("definition::{}", item.sig.ident),
                item.sig.ident.span(),
            );
        }
        let qualified = format!("{}::{}", self.symbol, item.sig.ident);
        let previous = std::mem::replace(&mut self.symbol, qualified);
        syn::visit::visit_impl_item_fn(self, item);
        self.symbol = previous;
    }
    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expr.func.as_ref() {
            let parts: Vec<_> = path
                .path
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect();
            if let Some(api) = [
                "ShellExecuteW",
                "ShellExecuteExW",
                "AuthorizationExecuteWithPrivileges",
            ]
            .iter()
            .find(|api| {
                parts.last().is_some_and(|last| {
                    *last == **api
                        || self
                            .native_aliases
                            .get(last)
                            .is_some_and(|canonical| canonical == **api)
                })
            }) {
                self.record_reference(&format!("native-api::{api}"), path.path.span());
            }
            let constructor = parts.len() >= 2
                && parts.last().is_some_and(|part| part == "new")
                && (parts.windows(2).any(|pair| pair == ["process", "Command"])
                    || parts
                        .get(parts.len() - 2)
                        .is_some_and(|alias| self.aliases.contains(alias)));
            if constructor {
                self.record("Command::new", expr.func.span());
            }
            if parts.iter().any(|part| part == "ManagedChild")
                && parts.last().is_some_and(|part| {
                    matches!(
                        part.as_str(),
                        "spawn" | "spawn_with_options" | "spawn_with_toolchain"
                    )
                })
            {
                self.record(
                    &format!("ManagedChild::{}", parts.last().unwrap()),
                    expr.func.span(),
                );
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }
    fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
        let joined = expr
            .path
            .segments
            .iter()
            .map(|part| part.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        for reference in [
            "PreparedExecutionEnvironment::prepare",
            "PreparedExecutionEnvironment::native_command",
            "ProfilePrepareScope::TrustedProvider",
            "run_prepare_command",
        ] {
            if joined == reference || joined.ends_with(reference) {
                self.record_reference(reference, expr.path.span());
            }
        }
        syn::visit::visit_expr_path(self, expr);
    }
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let method = expr.method.to_string();
        if method == "openApplicationAtURL_configuration_completionHandler" {
            self.record_reference(
                "native-api::openApplicationAtURL_configuration_completionHandler",
                expr.method.span(),
            );
        }
        if method == "native_command" {
            self.record_reference("method::native_command", expr.method.span());
        }
        if [
            "spawn",
            "spawn_with_options",
            "spawn_with_toolchain",
            "into_command",
            "output",
            "status",
            "wait_with_output",
            "exec",
        ]
        .contains(&method.as_str())
        {
            self.record(&format!("method::{method}"), expr.method.span());
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let tokens = macro_tokens_without_literals(mac.tokens.clone()).to_string();
        if mac.path.is_ident("include") {
            self.scan.diagnostics.insert(format!(
                "{}::{} has unresolved {}! source inclusion",
                self.file,
                self.symbol,
                mac.path.segments.last().unwrap().ident
            ));
        }
        if self.macro_has_process_launch(&tokens) {
            self.scan.diagnostics.insert(format!(
                "{}::{} has unresolved process-like macro tokens requiring exact review: {tokens}",
                self.file, self.symbol
            ));
        }
        if self.macro_has_native_api(&tokens) {
            self.scan.diagnostics.insert(format!(
                "{}::{} has unresolved enumerated native API macro tokens requiring exact review: {tokens}",
                self.file, self.symbol
            ));
        }
        syn::visit::visit_macro(self, mac);
    }
}

fn qualify(parent: &str, child: &str) -> String {
    format!("{parent}::{child}")
}
fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn module_paths(
    base: &Path,
    source_dir: &Path,
    name: &str,
    attrs: &[Attribute],
    diagnostics: &mut BTreeSet<String>,
) -> Vec<(PathBuf, PathBuf)> {
    let mut paths = Vec::new();
    let mut use_default = true;
    for attr in attrs {
        if attr.path().is_ident("path") {
            match &attr.meta {
                Meta::NameValue(value) => match &value.value {
                    Expr::Lit(literal) => match &literal.lit {
                        syn::Lit::Str(path) => {
                            paths.push(source_dir.join(path.value()));
                            use_default = false;
                        }
                        _ => {
                            diagnostics
                                .insert(format!("non-string #[path] under {}", base.display()));
                        }
                    },
                    _ => {
                        diagnostics.insert(format!("non-literal #[path] under {}", base.display()));
                    }
                },
                _ => {
                    diagnostics.insert(format!("unsupported #[path] under {}", base.display()));
                }
            }
        }
        if attr.path().is_ident("cfg_attr") {
            match attr.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::token::Comma>::parse_terminated,
            ) {
                Ok(values) => {
                    if let Some(condition) = values.first() {
                        let condition = cfg_truth(condition);
                        for value in values
                            .iter()
                            .skip(1)
                            .filter(|value| value.path().is_ident("path"))
                        {
                            if let Meta::NameValue(path) = value {
                                if let Expr::Lit(literal) = &path.value {
                                    if let syn::Lit::Str(path) = &literal.lit {
                                        if condition != Truth::False {
                                            paths.push(source_dir.join(path.value()));
                                            if condition == Truth::True {
                                                use_default = false;
                                            }
                                        }
                                    } else {
                                        diagnostics.insert(format!(
                                            "non-string cfg_attr #[path] under {}",
                                            base.display()
                                        ));
                                    }
                                } else {
                                    diagnostics.insert(format!(
                                        "non-literal cfg_attr #[path] under {}",
                                        base.display()
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    diagnostics.insert(format!(
                        "invalid cfg_attr under {}: {error}",
                        base.display()
                    ));
                }
            }
        }
    }
    if use_default {
        paths.extend([
            base.join(format!("{name}.rs")),
            base.join(name).join("mod.rs"),
        ]);
    }
    paths.sort();
    paths.dedup();
    let found: Vec<_> = paths
        .into_iter()
        .filter(|path| path.is_file())
        .map(|path| {
            let dir = if path.file_name().is_some_and(|leaf| leaf == "mod.rs") {
                path.parent().unwrap().to_path_buf()
            } else {
                base.join(name)
            };
            (path, dir)
        })
        .collect();
    if found.is_empty() {
        diagnostics.insert(format!("unresolved module {name} under {}", base.display()));
    }
    found
}

fn scan_file(
    root: &Path,
    file: &Path,
    module_dir: &Path,
    target: &str,
    module: &str,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scan: &mut Scan,
) {
    let file = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if !visited.insert((file.clone(), target.to_owned())) {
        return;
    }
    let source = match fs::read_to_string(&file) {
        Ok(source) => source,
        Err(error) => {
            scan.diagnostics
                .insert(format!("cannot read {}: {error}", file.display()));
            return;
        }
    };
    let origin_path = repo_relative(root, &file);
    let digest = Sha256::digest(source.as_bytes());
    let origin = scan
        .production_origins
        .entry(origin_path)
        .or_insert_with(|| ProductionOriginSource {
            sha256: format!("{digest:x}"),
            targets: BTreeSet::new(),
        });
    origin.targets.insert(target.to_owned());
    let syntax = match syn::parse_file(&source) {
        Ok(syntax) => syntax,
        Err(error) => {
            scan.diagnostics
                .insert(format!("cannot parse {}: {error}", file.display()));
            return;
        }
    };
    scan_items(
        root,
        &file,
        module_dir,
        target,
        module,
        &syntax.items,
        visited,
        scan,
    );
}

fn scan_items(
    root: &Path,
    file: &Path,
    module_dir: &Path,
    target: &str,
    module: &str,
    items: &[Item],
    visited: &mut BTreeSet<(PathBuf, String)>,
    scan: &mut Scan,
) {
    let aliases = aliases_for(items);
    let native_aliases = native_aliases_for(items);
    let file_name = repo_relative(root, file);
    for item in items {
        if !attrs_possible(item_attrs(item)) {
            continue;
        }
        if let Item::Mod(child) = item {
            let name = child.ident.to_string();
            let child_module = qualify(module, &name);
            if let Some((_, nested)) = &child.content {
                scan_items(
                    root,
                    file,
                    &module_dir.join(&name),
                    target,
                    &child_module,
                    nested,
                    visited,
                    scan,
                );
            } else {
                for (path, dir) in module_paths(
                    module_dir,
                    file.parent().unwrap_or(module_dir),
                    &name,
                    &child.attrs,
                    &mut scan.diagnostics,
                ) {
                    scan_file(root, &path, &dir, target, &child_module, visited, scan);
                }
            }
            continue;
        }
        let mut visitor = Visitor {
            file: &file_name,
            target,
            aliases: aliases.clone(),
            native_aliases: native_aliases.clone(),
            symbol: module.into(),
            scan,
        };
        match item {
            Item::Impl(implementation) => {
                visitor.symbol = qualify(
                    module,
                    &implementation
                        .self_ty
                        .to_token_stream()
                        .to_string()
                        .replace(' ', ""),
                );
                visitor.visit_item_impl(implementation);
            }
            Item::Trait(trait_item) => {
                visitor.symbol = qualify(module, &trait_item.ident.to_string());
                visitor.visit_item_trait(trait_item);
            }
            _ => visitor.visit_item(item),
        }
    }
}

fn bounded_metadata(root: &Path, manifest: &Path) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let child = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn cargo metadata: {error}"))?;
    let (status, stdout, stderr) = bounded_child_output(
        child,
        deadline,
        "cargo metadata",
        4 * 1024 * 1024,
        1024 * 1024,
    )?;
    if !status.success() {
        return Err(format!(
            "cargo metadata failed for {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&stderr)
        ));
    }
    Ok(stdout)
}

fn bounded_child_output(
    mut child: Child,
    deadline: Instant,
    label: &str,
    stdout_cap: usize,
    stderr_cap: usize,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), String> {
    let mut stdout = child.stdout.take().ok_or("metadata stdout pipe missing")?;
    let mut stderr = child.stderr.take().ok_or("metadata stderr pipe missing")?;
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_tx.send(read_capped(&mut stdout, stdout_cap));
    });
    thread::spawn(move || {
        let _ = stderr_tx.send(read_capped(&mut stderr, stderr_cap));
    });
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| format!("wait cargo metadata: {error}"))?
        {
            Some(status) => break status,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                while Instant::now() < deadline {
                    if child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                return Err(format!(
                    "{label} direct child exceeded the total 30s budget"
                ));
            }
        }
    };
    let manifest = Path::new(label);
    let (stdout, stdout_truncated) =
        receive_metadata_stream(&stdout_rx, deadline, "stdout", manifest)?;
    let (stderr, stderr_truncated) =
        receive_metadata_stream(&stderr_rx, deadline, "stderr", manifest)?;
    if stdout_truncated || stderr_truncated {
        return Err(format!("{label} output exceeded cap"));
    }
    Ok((status, stdout, stderr))
}

fn read_capped(reader: &mut impl Read, cap: usize) -> Result<(Vec<u8>, bool), String> {
    let mut output = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("read metadata pipe: {error}"))?;
        if count == 0 {
            break;
        }
        let available = cap.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..count.min(available)]);
        truncated |= count > available;
    }
    Ok((output, truncated))
}

fn receive_metadata_stream(
    receiver: &std::sync::mpsc::Receiver<Result<(Vec<u8>, bool), String>>,
    deadline: Instant,
    stream: &str,
    manifest: &Path,
) -> Result<(Vec<u8>, bool), String> {
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            format!(
                "{stream} drain exceeded the shared process deadline: {}",
                manifest.display()
            )
        })?
}

#[test]
fn p1b_pipe_holder_grandchild_fixture() {
    if env::var_os("WEBCODEX_P1B_HOLD_PIPE").is_some() {
        thread::sleep(Duration::from_millis(800));
    }
}

#[test]
fn p1b_pipe_holder_child_fixture() {
    if env::var_os("WEBCODEX_P1B_SPAWN_PIPE_HOLDER").is_none() {
        return;
    }
    Command::new(env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "p1b_pipe_holder_grandchild_fixture",
            "--nocapture",
        ])
        .env("WEBCODEX_P1B_HOLD_PIPE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("grandchild holding both captured pipes must start");
}

#[test]
#[ignore = "timing-sensitive held-pipe deadline negative control; run explicitly with --ignored --exact --test-threads=1"]
fn p1b_metadata_deadline_returns_when_grandchild_holds_stdout_and_stderr() {
    let child = Command::new(env::current_exe().expect("test executable path"))
        .args(["--exact", "p1b_pipe_holder_child_fixture", "--nocapture"])
        .env("WEBCODEX_P1B_SPAWN_PIPE_HOLDER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fixture child must start");
    let started = Instant::now();
    let result = bounded_child_output(
        child,
        started + Duration::from_millis(150),
        "descendant pipe fixture",
        4096,
        4096,
    );
    assert!(
        result
            .unwrap_err()
            .contains("stdout drain exceeded the shared process deadline"),
        "a descendant-held output pipe must be reported as bounded drain failure"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the actual held-pipe fixture must not wait for the grandchild to exit"
    );
}

fn targets_from_manifest(root: &Path, manifest: &Path) -> Vec<(String, String, PathBuf)> {
    let metadata = bounded_metadata(root, manifest).unwrap_or_else(|error| panic!("{error}"));
    let json: serde_json::Value = serde_json::from_slice(&metadata).expect("metadata JSON");
    let mut result = Vec::new();
    for package in json["packages"].as_array().expect("packages") {
        for target in package["targets"].as_array().expect("targets") {
            let kinds: Vec<_> = target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect();
            if !kinds.iter().any(|kind| {
                matches!(
                    *kind,
                    "lib" | "bin" | "cdylib" | "staticlib" | "proc-macro" | "custom-build"
                )
            }) {
                continue;
            }
            let path = PathBuf::from(target["src_path"].as_str().unwrap());
            if path.starts_with(root) {
                result.push((
                    package["name"].as_str().unwrap().into(),
                    target["name"].as_str().unwrap().into(),
                    path,
                ));
            }
        }
    }
    result.sort();
    result
}

fn production_targets(root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut manifests = vec![root.join("Cargo.toml")];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(".git" | "target" | "node_modules")
                ) {
                    stack.push(path);
                }
            } else if path.file_name().is_some_and(|name| name == "Cargo.toml")
                && path != root.join("Cargo.toml")
            {
                manifests.push(path);
            }
        }
    }
    manifests.sort();
    manifests.dedup();
    let mut result = Vec::new();
    for manifest in manifests {
        result.extend(targets_from_manifest(root, &manifest));
    }
    result.sort();
    result.dedup();
    result
}

fn scan_repository(root: &Path) -> Scan {
    let mut scan = Scan::default();
    let mut visited = BTreeSet::new();
    for (package, target, file) in production_targets(root) {
        let target = format!("{package}:{target}");
        scan_file(
            root,
            &file,
            file.parent().unwrap(),
            &target,
            "crate",
            &mut visited,
            &mut scan,
        );
    }
    scan
}

#[test]
fn p1b_all_production_targets_are_discoverable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let targets = production_targets(&root);
    assert!(
        targets
            .iter()
            .any(|(package, _, _)| package == "webcodex-desktop"),
        "workspace-excluded Desktop production targets must stay in the inventory"
    );
    let scan = scan_repository(&root);
    assert!(
        scan.diagnostics.is_empty(),
        "unresolved production boundary:\n{}",
        scan.diagnostics
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        !scan.sites.is_empty(),
        "production targets must yield launch candidates"
    );
    let (inventory, overlay_status) = candidate_inventory();
    require_accepted_inventory(&inventory);
    let raw_expected = accepted_raw_sites(&inventory);
    compare_raw(
        &raw_expected,
        &scan.sites.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap_or_else(|error| panic!("accepted production raw inventory drift: {error}"));
    assert_eq!(
        accepted_references(&inventory),
        scan.references.values().cloned().collect::<Vec<_>>(),
        "accepted production reference inventory drift"
    );
    eprintln!("P1C_C2_PRODUCTION_SCANNER_RAW_DELTA=10_REFERENCE_DELTA=0");
    compare_production_origins(&inventory, &scan.production_origins)
        .unwrap_or_else(|error| panic!("accepted production origin closure drift: {error}"));
    eprintln!("P1C_C2_FROZEN_PRODUCTION_ORIGIN_CLOSURE_MATCH=PASS");
    assert_eq!(
        overlay_status, "SOL_REVIEWED_C2_OVERLAY_ACCEPTED",
        "NOT_RUN_PENDING_REVIEW: C2 candidate overlay has not received independent review"
    );
    let expected_targets: BTreeSet<_> = inventory["rust_targets"]
        .as_array()
        .expect("accepted rust_targets array")
        .iter()
        .map(|row| {
            (
                string_field(row, "package").to_owned(),
                string_field(row, "target").to_owned(),
                string_field(row, "file").to_owned(),
            )
        })
        .collect();
    let actual_targets: BTreeSet<_> = targets
        .iter()
        .map(|(package, target, file)| {
            (package.clone(), target.clone(), repo_relative(&root, file))
        })
        .collect();
    assert_eq!(
        expected_targets, actual_targets,
        "production target list drift"
    );
    eprintln!("P1B_RUST_RAW_SITE_COUNT={}", scan.sites.len());
    if env::var_os("WEBCODEX_P1B_DUMP_RAW").is_some() {
        for (package, target, source) in targets {
            eprintln!(
                "P1B_TARGET\t{package}\t{target}\t{}",
                repo_relative(&root, &source)
            );
        }
        for site in scan.sites.values() {
            eprintln!(
                "P1B_RAW\t{}\t{}\t{}\t{}\ttargets={:?}\tclass=UNCLASSIFIED",
                site.file, site.symbol, site.primitive, site.count, site.targets
            );
        }
        for reference in scan.references.values() {
            eprintln!(
                "P1B_REFERENCE\t{}\t{}\t{}\t{}\ttargets={:?}",
                reference.file,
                reference.symbol,
                reference.reference,
                reference.count,
                reference.targets
            );
        }
        for diagnostic in &scan.diagnostics {
            eprintln!("P1B_DIAGNOSTIC\t{diagnostic}");
        }
    }
}

fn ref_rows<'a>(scan: &'a Scan, reference: &str) -> Vec<&'a SymbolReference> {
    scan.references
        .values()
        .filter(|row| row.reference == reference)
        .collect()
}

#[test]
fn p1b_profile_prepare_symbols_have_fixed_production_origins() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let scan = scan_repository(&root);
    assert!(
        scan.diagnostics.is_empty(),
        "unresolved production boundary: {:?}",
        scan.diagnostics
    );

    let prepare = ref_rows(&scan, "PreparedExecutionEnvironment::prepare");
    let prepare_origins: BTreeSet<_> = prepare
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        prepare_origins,
        BTreeSet::from([(
            "crates/webcodex-runner/src/webcodex_runner/plugin.rs",
            "crate::webcodex_runner::plugin::prepare_provider",
            1
        )])
    );
    let native_command = ref_rows(&scan, "method::native_command");
    let native_origins: BTreeSet<_> = native_command
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        native_origins,
        BTreeSet::from([(
            "crates/webcodex-runner/src/webcodex_runner/plugin.rs",
            "crate::webcodex_runner::plugin::prepare_provider",
            1
        )])
    );

    let scope = ref_rows(&scan, "ProfilePrepareScope::TrustedProvider");
    let scope_origins: BTreeSet<_> = scope
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        scope_origins,
        BTreeSet::from([(
            "crates/webcodex-runner/src/webcodex_runner/shell.rs",
            "crate::webcodex_runner::shell::PreparedExecutionEnvironment::prepare",
            1
        )])
    );

    let prepare_method = ref_rows(&scan, "definition::prepare");
    assert_eq!(prepare_method.len(), 1);
    let native_method = ref_rows(&scan, "definition::native_command");
    assert_eq!(native_method.len(), 1);

    let run_prepare = ref_rows(&scan, "run_prepare_command");
    let origins: BTreeSet<_> = run_prepare
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        origins,
        BTreeSet::from([
            (
                "crates/webcodex-runner/src/webcodex_runner/shell.rs",
                "crate::webcodex_runner::shell::configured_script_runtime_plan",
                1
            ),
            (
                "crates/webcodex-runner/src/webcodex_runner/shell.rs",
                "crate::webcodex_runner::shell::capture_profile_env_snapshot",
                1
            ),
        ])
    );
}

#[test]
fn p1b_platform_handoff_sites_are_exact_and_named() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let scan = scan_repository(&root);
    let shell_execute: BTreeSet<_> = ref_rows(&scan, "native-api::ShellExecuteW")
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        shell_execute,
        BTreeSet::from([(
            "apps/desktop/src-tauri/src/platform/opener.rs",
            "crate::platform::opener::open_literal",
            1
        )])
    );
    let authorization: BTreeSet<_> =
        ref_rows(&scan, "native-api::AuthorizationExecuteWithPrivileges")
            .iter()
            .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
            .collect();
    assert_eq!(
        authorization,
        BTreeSet::from([(
            "apps/desktop/src-tauri/src/updates/install/native.rs",
            "crate::updates::install::native::macos::launch",
            1
        )])
    );
    let open_application: BTreeSet<_> = ref_rows(
        &scan,
        "native-api::openApplicationAtURL_configuration_completionHandler",
    )
    .iter()
    .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
    .collect();
    assert_eq!(
        open_application,
        BTreeSet::from([(
            "crates/webcodex-computer/src/platform/macos/applications.rs",
            "crate::platform::macos::applications::launch_application",
            1,
        )]),
    );
    let shell_execute_ex: BTreeSet<_> = ref_rows(&scan, "native-api::ShellExecuteExW")
        .iter()
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        shell_execute_ex,
        BTreeSet::from([
            (
                "crates/webcodex-environment/src/privilege.rs",
                "crate::privilege::elevate_windows",
                1
            ),
            (
                "crates/webcodex-computer/src/platform/windows/applications.rs",
                "crate::platform::windows::applications::launch_application",
                1
            ),
        ])
    );

    let exec: BTreeSet<_> = scan
        .sites
        .values()
        .filter(|row| row.primitive == "method::exec")
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        exec,
        BTreeSet::from([
            (
                "crates/webcodex-cli/src/webcodex_cli/service.rs",
                "crate::webcodex_cli::service::run_internal_binary",
                1
            ),
            (
                "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
                "crate::webcodex_runner::persistent_shell::PersistentShellManager::exec",
                1
            ),
            (
                "crates/webcodex-runner/src/webcodex_runner/persistent_shell.rs",
                "crate::webcodex_runner::persistent_shell::PersistentShellManager::handle_operation",
                1
            ),
        ])
    );

    let toolchain_spawn: BTreeSet<_> = scan
        .sites
        .values()
        .filter(|row| row.primitive == "method::spawn_with_toolchain")
        .map(|row| (row.file.as_str(), row.symbol.as_str(), row.count))
        .collect();
    assert_eq!(
        toolchain_spawn,
        BTreeSet::from([
            (
                "crates/webcodex-lsp/src/supervisor.rs",
                "crate::supervisor::LspCommand::spawn",
                1
            ),
            (
                "crates/webcodex-process/src/bin/seatbelt_ae_probe.rs",
                "crate::run",
                1
            ),
            (
                "crates/webcodex-process/src/execution_broker/mod.rs",
                "crate::execution_broker::ExecutionBroker::spawn",
                1
            ),
            (
                "crates/webcodex-runner/src/webcodex_runner/local_execution.rs",
                "crate::webcodex_runner::local_execution::spawn_local_action",
                1
            ),
            (
                "crates/webcodex-workspace/src/git_broker.rs",
                "crate::git_broker::BrokeredGit::spawn",
                1
            ),
        ]),
        "broker.spawn_with_toolchain instance callers must remain in the exact origin inventory"
    );
}

struct ProbeArgVisitor {
    args: Vec<String>,
}

#[derive(Default)]
struct ScopeMatchVisitor {
    arms: BTreeMap<String, String>,
}

impl<'ast> Visit<'ast> for ScopeMatchVisitor {
    fn visit_expr_match(&mut self, expr: &'ast syn::ExprMatch) {
        if matches!(expr.expr.as_ref(), Expr::Path(path) if path.path.is_ident("scope")) {
            for arm in &expr.arms {
                let pattern = arm.pat.to_token_stream().to_string();
                let body = arm.to_token_stream().to_string();
                let digest = Sha256::digest(body.as_bytes());
                self.arms.insert(pattern, format!("{digest:x}"));
            }
        }
        syn::visit::visit_expr_match(self, expr);
    }
}
impl<'ast> Visit<'ast> for ProbeArgVisitor {
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if expr.method == "arg"
            && matches!(expr.receiver.as_ref(), Expr::Path(path) if path.path.is_ident("probe_blueprint"))
        {
            let value = expr
                .args
                .first()
                .and_then(|arg| match arg {
                    Expr::Lit(literal) => match &literal.lit {
                        syn::Lit::Str(value) => Some(value.value()),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap_or_else(|| "<non-literal>".into());
            self.args.push(value);
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

#[test]
fn p1b_configured_node_probe_keeps_fixed_version_payload() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let source =
        fs::read_to_string(root.join("crates/webcodex-runner/src/webcodex_runner/shell.rs"))
            .unwrap();
    let syntax = syn::parse_file(&source).unwrap();
    let function = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "configured_script_runtime_plan" => {
                Some(function)
            }
            _ => None,
        })
        .expect("production probe function exists");
    assert!(
        attrs_possible(&function.attrs),
        "fixed probe origin must be production compiled"
    );
    let mut visitor = ProbeArgVisitor { args: Vec::new() };
    visitor.visit_block(&function.block);
    assert_eq!(
        visitor.args,
        ["--version"],
        "the fixed Node probe payload changed; review it explicitly"
    );
}

#[test]
fn p1b_trusted_profile_prepare_dispatch_keeps_exact_branch_fingerprints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let source =
        fs::read_to_string(root.join("crates/webcodex-runner/src/webcodex_runner/shell.rs"))
            .unwrap();
    let syntax = syn::parse_file(&source).unwrap();
    let function = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "capture_profile_env_snapshot" => {
                Some(function)
            }
            _ => None,
        })
        .expect("production profile prepare dispatcher exists");
    assert!(
        attrs_possible(&function.attrs),
        "profile dispatcher must remain production compiled"
    );
    let mut visitor = ScopeMatchVisitor::default();
    visitor.visit_block(&function.block);
    if env::var_os("WEBCODEX_P1B_DUMP_SCOPE_ARMS").is_some() {
        for (pattern, digest) in &visitor.arms {
            eprintln!("P1B_SCOPE_ARM\t{pattern}\t{digest}");
        }
        assert_eq!(
            visitor.arms.len(),
            2,
            "capture_profile_env_snapshot must keep exactly the two reviewed scopes"
        );
        return;
    }
    assert_eq!(
        visitor.arms,
        BTreeMap::from([
            (
                "ProfilePrepareScope :: TrustedProvider".into(),
                "51123ec1530ed5a911c182d02ce9fb7ff77aece17724c18f38a3aa7cc936cd71".into(),
            ),
            (
                "ProfilePrepareScope :: RegisteredWorkspace { registry_dir }".into(),
                "6f8e90e8be04be44f9cbcca3b031e9e6993fed65376e4b5f6004479a5b85f154".into(),
            ),
        ]),
        "profile scope dispatch branches changed; inspect both authority paths before updating the fixed fingerprints"
    );
}

fn files_with_extensions(root: &Path, dir: &Path, extensions: &[&str]) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
            {
                files.insert(repo_relative(root, &path));
            }
        }
    }
    files
}

fn all_regular_files(root: &Path, dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current).map_err(|error| {
            format!(
                "cannot enumerate publish directory {}: {error}",
                current.display()
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "cannot read publish directory entry under {}: {error}",
                    current.display()
                )
            })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| {
                format!("cannot inspect publish path {}: {error}", path.display())
            })?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                files.insert(repo_relative(root, &path));
            }
        }
    }
    Ok(files)
}

fn add_tree_files(root: &Path, relative: &str, extensions: &[&str], files: &mut BTreeSet<String>) {
    files.extend(files_with_extensions(
        root,
        &root.join(relative),
        extensions,
    ));
}

fn tsconfig_include_roots(config: &serde_json::Value) -> Vec<(PathBuf, Option<String>)> {
    config["include"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|pattern| {
            let path = Path::new(pattern);
            let prefix = pattern
                .find('*')
                .map(|index| &pattern[..index])
                .unwrap_or(pattern);
            let dir = Path::new(prefix).parent().unwrap_or(Path::new("."));
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_owned);
            (dir.to_path_buf(), extension)
        })
        .collect()
}

fn add_local_import_closure(root: &Path, entry: &Path, files: &mut BTreeSet<String>) {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut pending = vec![entry.to_path_buf()];
    let mut visited = BTreeSet::new();
    while let Some(path) = pending.pop() {
        let path = fs::canonicalize(&path).unwrap_or(path);
        let Ok(relative_path) = path.strip_prefix(&root) else {
            continue;
        };
        let relative = relative_path.to_path_buf();
        if !visited.insert(relative.clone()) {
            continue;
        }
        let Ok(source) = fs::read_to_string(root.join(&relative)) else {
            continue;
        };
        files.insert(repo_relative(&root, &root.join(&relative)));
        let mut cursor = 0;
        while let Some(found) = source[cursor..].find("import") {
            let start = cursor + found + "import".len();
            cursor = start;
            let bytes = source.as_bytes();
            let mut quote = None;
            let mut index = start;
            while index < bytes.len() {
                if bytes[index] == b'\'' || bytes[index] == b'"' {
                    quote = Some(bytes[index]);
                    break;
                }
                if bytes[index] == b'\n' || bytes[index] == b';' {
                    break;
                }
                index += 1;
            }
            let Some(quote) = quote else {
                continue;
            };
            let value_start = index + 1;
            let Some(end) = bytes[value_start..].iter().position(|byte| *byte == quote) else {
                continue;
            };
            let specifier = &source[value_start..value_start + end];
            cursor = value_start + end + 1;
            if !specifier.starts_with('.') {
                continue;
            }
            let base = root.join(&relative).parent().unwrap().join(specifier);
            let mut resolved = None;
            for candidate in [
                base.clone(),
                base.with_extension("ts"),
                base.with_extension("tsx"),
                base.with_extension("js"),
                base.with_extension("jsx"),
                base.with_extension("json"),
                base.with_extension("css"),
                base.join("index.ts"),
                base.join("index.tsx"),
            ] {
                if candidate.is_file() {
                    resolved = Some(candidate);
                    break;
                }
            }
            if let Some(resolved) = resolved {
                pending.push(resolved);
            }
        }
    }
}

fn non_rust_release_assets(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files: BTreeSet<String> = BTreeSet::new();
    let web_package: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("npm/webcodex/package.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    files.insert("npm/webcodex/package.json".to_string());
    for entry in web_package["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
    {
        let path = root.join("npm/webcodex").join(entry);
        if path.is_dir() {
            files.extend(all_regular_files(root, &path)?);
        } else if path.is_file() {
            files.insert(repo_relative(root, &path));
        }
    }

    let sdk = "npm/plugin-sdk";
    for name in [
        "package.json",
        "tsconfig.json",
        "tsconfig.typecheck.json",
        "README.md",
        "README.zh-CN.md",
    ] {
        let path = root.join(sdk).join(name);
        if path.is_file() {
            files.insert(repo_relative(root, &path));
        }
    }
    add_tree_files(root, "npm/plugin-sdk/src", &["ts"], &mut files);
    add_tree_files(root, "npm/plugin-sdk/examples", &["ts"], &mut files);

    for entry in fs::read_dir(root.join("plugins"))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let package_dir = entry.path();
        if !package_dir.is_dir() {
            continue;
        }
        for name in ["package.json", "tsconfig.json"] {
            let path = package_dir.join(name);
            if path.is_file() {
                files.insert(repo_relative(root, &path));
            }
        }
        let config_path = package_dir.join("tsconfig.json");
        if config_path.is_file() {
            let config: serde_json::Value =
                serde_json::from_slice(&fs::read(&config_path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            for (dir, extension) in tsconfig_include_roots(&config) {
                let dir = package_dir.join(dir);
                let extensions: Vec<_> = extension.as_deref().unwrap_or("ts").split(',').collect();
                files.extend(files_with_extensions(root, &dir, &extensions));
            }
        }
        let scripts = package_dir.join("scripts");
        if scripts.is_dir() {
            files.extend(files_with_extensions(root, &scripts, &["js", "mjs", "ts"]));
        }
    }

    for name in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
    ] {
        let path = root.join("apps/desktop").join(name);
        if path.is_file() {
            files.insert(repo_relative(root, &path));
        }
    }
    add_tree_files(
        root,
        "apps/desktop/scripts",
        &["js", "mjs", "ts"],
        &mut files,
    );
    let tauri_config = root.join("apps/desktop/src-tauri/tauri.conf.json");
    if tauri_config.is_file() {
        files.insert(repo_relative(root, &tauri_config));
    }
    let publication_hook = root.join("npm/webcodex/test/release-manifest-check.js");
    if publication_hook.is_file() {
        files.insert(repo_relative(root, &publication_hook));
    }
    add_local_import_closure(root, &root.join("apps/desktop/src/main.tsx"), &mut files);
    let frontend = root.join("frontend/src/ui/BrandMark.tsx");
    if frontend.is_file() {
        add_local_import_closure(root, &frontend, &mut files);
    }
    Ok(files)
}

fn non_rust_fingerprints(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut fingerprints = BTreeMap::new();
    for path in non_rust_release_assets(root)? {
        let bytes = fs::read(root.join(&path)).map_err(|error| format!("{path}: {error}"))?;
        let digest = Sha256::digest(bytes);
        fingerprints.insert(path, format!("{digest:x}"));
    }
    Ok(fingerprints)
}

fn accepted_inventory() -> serde_json::Value {
    let (inventory, status) = candidate_inventory();
    assert_eq!(
        status, "SOL_REVIEWED_C2_OVERLAY_ACCEPTED",
        "NOT_RUN_PENDING_REVIEW: C2 candidate overlay has not received independent review"
    );
    inventory
}

fn candidate_inventory() -> (serde_json::Value, String) {
    let application = p1c_c2_overlay::apply_sparse_overlay(
        include_str!("../../../research/implementation/p1b/launch-inventory.json"),
        include_str!("../../../research/implementation/p1c/c2-guard-overlay.json"),
    )
    .expect("C2 overlay must be structurally bound to accepted P1B inventory");
    (application.inventory, application.status)
}

fn string_field<'a>(row: &'a serde_json::Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("inventory {key} must be a string"))
}

fn targets_field(row: &serde_json::Value) -> BTreeSet<String> {
    row["targets"]
        .as_array()
        .expect("inventory targets array")
        .iter()
        .map(|target| target.as_str().expect("target string").to_owned())
        .collect()
}

fn accepted_raw_sites(inventory: &serde_json::Value) -> Vec<RawSite> {
    inventory["raw_sites"]
        .as_array()
        .expect("accepted raw_sites array")
        .iter()
        .map(|row| RawSite {
            file: string_field(row, "file").to_owned(),
            symbol: string_field(row, "symbol").to_owned(),
            primitive: string_field(row, "primitive").to_owned(),
            count: row["count"].as_u64().expect("count integer") as usize,
            targets: targets_field(row),
        })
        .collect()
}

fn accepted_references(inventory: &serde_json::Value) -> Vec<SymbolReference> {
    inventory["references"]
        .as_array()
        .expect("accepted references array")
        .iter()
        .filter(|row| {
            !row["reference"]
                .as_str()
                .is_some_and(|value| value.starts_with("boundary-call::"))
        })
        .map(|row| SymbolReference {
            file: string_field(row, "file").to_owned(),
            symbol: string_field(row, "symbol").to_owned(),
            reference: string_field(row, "reference").to_owned(),
            targets: targets_field(row),
            count: row["count"].as_u64().expect("count integer") as usize,
        })
        .collect()
}

fn accepted_asset_fingerprints(inventory: &serde_json::Value) -> BTreeMap<String, String> {
    inventory["nonrust_assets"]
        .as_array()
        .expect("accepted nonrust_assets array")
        .iter()
        .map(|row| {
            (
                string_field(row, "path").to_owned(),
                string_field(row, "sha256").to_owned(),
            )
        })
        .collect()
}

fn accepted_body_fingerprints(inventory: &serde_json::Value) -> BTreeMap<String, String> {
    inventory["body_fingerprints"]
        .as_array()
        .expect("accepted body_fingerprints array")
        .iter()
        .map(|row| {
            (
                format!(
                    "{}::{}",
                    string_field(row, "file"),
                    string_field(row, "symbol")
                ),
                string_field(row, "sha256").to_owned(),
            )
        })
        .collect()
}

fn accepted_boundary_references(inventory: &serde_json::Value) -> BTreeMap<String, usize> {
    inventory["references"]
        .as_array()
        .expect("accepted references array")
        .iter()
        .filter(|row| {
            row["reference"]
                .as_str()
                .is_some_and(|value| value.starts_with("boundary-call::"))
        })
        .map(|row| {
            let target = string_field(row, "reference").trim_start_matches("boundary-call::");
            (
                format!(
                    "{}::{} -> {target}",
                    string_field(row, "file"),
                    string_field(row, "symbol")
                ),
                row["count"].as_u64().expect("reference count") as usize,
            )
        })
        .collect()
}

fn compare_boundary_references(
    expected: &BTreeMap<String, usize>,
    actual: &BTreeMap<String, usize>,
) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "boundary reference drift; missing={:?}; added={:?}; changed={:?}",
            expected
                .keys()
                .filter(|key| !actual.contains_key(*key))
                .collect::<Vec<_>>(),
            actual
                .keys()
                .filter(|key| !expected.contains_key(*key))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .filter_map(|(key, count)| actual
                    .get(key)
                    .filter(|actual_count| *actual_count != count)
                    .map(|actual_count| (key, count, actual_count)))
                .collect::<Vec<_>>(),
        ))
    }
}

fn require_accepted_inventory(inventory: &serde_json::Value) {
    assert_eq!(
        string_field(inventory, "status"),
        "SOL_REVIEWED_CLASSIFICATION_ACCEPTED",
        "the guard refuses draft or unreviewed inventory data"
    );
    assert_eq!(
        string_field(inventory, "fingerprint_algorithm"),
        "rust_syn_quote_tokens_v1"
    );
}

fn compare_production_origins(
    expected: &serde_json::Value,
    actual: &BTreeMap<String, ProductionOriginSource>,
) -> Result<(), String> {
    let rows = expected["production_origin_sources"]
        .as_array()
        .ok_or_else(|| "accepted inventory has no production_origin_sources array".to_owned())?;
    let expected: BTreeMap<_, _> = rows
        .iter()
        .map(|row| {
            (
                string_field(row, "path").to_owned(),
                ProductionOriginSource {
                    sha256: string_field(row, "sha256").to_owned(),
                    targets: targets_field(row),
                },
            )
        })
        .collect();
    if &expected == actual {
        Ok(())
    } else {
        Err(format!(
            "production source origin drift; missing={:?}; added={:?}; changed={:?}",
            expected
                .keys()
                .filter(|path| !actual.contains_key(*path))
                .collect::<Vec<_>>(),
            actual
                .keys()
                .filter(|path| !expected.contains_key(*path))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .filter_map(|(path, value)| actual
                    .get(path)
                    .filter(|actual| *actual != value)
                    .map(|actual| (path, value, actual)))
                .collect::<Vec<_>>(),
        ))
    }
}

fn compile_fixture_rust(root: &Path, source: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), source).unwrap();
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let child = Command::new(rustc)
        .args(["--crate-type=lib", "--edition=2021"])
        .arg(root.join("src/lib.rs"))
        .arg("-o")
        .arg(root.join("libfixture.rlib"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fixture rustc must start");
    let (status, _stdout, stderr) = bounded_child_output(
        child,
        Instant::now() + Duration::from_secs(10),
        "compile P1B source-origin fixture",
        16 * 1024,
        16 * 1024,
    )
    .expect("fixture rustc must complete within the shared deadline");
    assert!(
        status.success(),
        "fixture must compile as production Rust: {}",
        String::from_utf8_lossy(&stderr)
    );
}

fn scan_fixture_rust(root: &Path) -> Scan {
    let root = fs::canonicalize(root).unwrap();
    let mut scan = Scan::default();
    scan_file(
        &root,
        &root.join("src/lib.rs"),
        &root.join("src"),
        "fixture:lib",
        "crate",
        &mut BTreeSet::new(),
        &mut scan,
    );
    scan
}

#[test]
fn p1b_compiled_existing_helper_reuse_is_red_by_production_origin_json_gate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let inventory = accepted_inventory();
    require_accepted_inventory(&inventory);
    let accepted_raw = accepted_raw_sites(&inventory);
    let accepted_refs = accepted_references(&inventory);
    let production = scan_repository(&root);
    assert!(
        production.diagnostics.is_empty(),
        "current production scanner must be clean"
    );
    compare_production_origins(&inventory, &production.production_origins).unwrap();
    compare_raw(
        &accepted_raw,
        &production.sites.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap();
    compare_references(
        &accepted_refs,
        &production.references.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap();

    let baseline_root = tempfile::tempdir().unwrap();
    let base_source =
        "pub fn fixed_probe() { let _ = std::process::Command::new(\"fixed\").output(); }\n";
    compile_fixture_rust(baseline_root.path(), base_source);
    let base = scan_fixture_rust(baseline_root.path());
    assert!(base.diagnostics.is_empty(), "baseline fixture resolves");
    let fixture_inventory = serde_json::json!({
        "production_origin_sources": base.production_origins.iter().map(|(path, origin)| {
            serde_json::json!({"path": path, "sha256": origin.sha256, "targets": origin.targets})
        }).collect::<Vec<_>>()
    });
    assert_eq!(
        compare_production_origins(&fixture_inventory, &base.production_origins),
        Ok(())
    );

    let changed_root = tempfile::tempdir().unwrap();
    fs::create_dir_all(changed_root.path().join("src")).unwrap();
    fs::write(
        changed_root.path().join("src/test_only.rs"),
        "pub fn test_only_launcher() { let _ = std::process::Command::new(\"test\").status(); }",
    )
    .unwrap();
    fs::write(
        changed_root.path().join("src/added_caller.rs"),
        "pub fn new_production_caller() { super::fixed_probe(); }",
    )
    .unwrap();
    let caller_source =
        format!("{base_source}#[cfg(not(test))] mod added_caller;\n#[cfg(test)] mod test_only;\n");
    compile_fixture_rust(changed_root.path(), &caller_source);
    let changed = scan_fixture_rust(changed_root.path());
    assert!(
        changed.diagnostics.is_empty(),
        "new caller fixture resolves"
    );
    let normalized_sites = |scan: &Scan| {
        scan.sites
            .values()
            .map(|row| {
                (
                    row.symbol.clone(),
                    row.primitive.clone(),
                    row.count,
                    row.targets.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    let normalized_refs = |scan: &Scan| {
        scan.references
            .values()
            .map(|row| {
                (
                    row.symbol.clone(),
                    row.reference.clone(),
                    row.count,
                    row.targets.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(normalized_sites(&base), normalized_sites(&base));
    assert_eq!(normalized_refs(&base), normalized_refs(&base));
    eprintln!("BASELINE_FIXTURE_RAW_REFERENCE_COMPARE_PASS");
    assert_eq!(
        normalized_sites(&base),
        normalized_sites(&changed),
        "the new caller adds no launch primitive"
    );
    assert_eq!(
        normalized_refs(&base),
        normalized_refs(&changed),
        "the fixed helper's raw/reference rows are unchanged"
    );
    eprintln!("NEW_CALLER_RAW_REFERENCE_GATE_STILL_PASS");
    assert!(changed.production_origins.contains_key("src/lib.rs"));
    assert!(changed
        .production_origins
        .contains_key("src/added_caller.rs"));
    assert!(
        !changed.production_origins.contains_key("src/test_only.rs"),
        "the separate cfg(test) source module is excluded from production origins"
    );
    let reuse_result = compare_production_origins(&fixture_inventory, &changed.production_origins);
    eprintln!("EXPECTED_COMPILED_HELPER_REUSE_RED\t{reuse_result:?}");
    assert!(
        reuse_result.is_err(),
        "same origin compare must be RED for a compiled new production caller"
    );
    let baseline_path = baseline_root.path().join("src/lib.rs");
    fs::write(
        &baseline_path,
        format!("{base_source}// changed source bytes\n"),
    )
    .unwrap();
    compile_fixture_rust(
        baseline_root.path(),
        &fs::read_to_string(&baseline_path).unwrap(),
    );
    let changed_bytes = scan_fixture_rust(baseline_root.path());
    let content_result =
        compare_production_origins(&fixture_inventory, &changed_bytes.production_origins);
    eprintln!("EXPECTED_SOURCE_CONTENT_CHANGE_RED\t{content_result:?}");
    assert!(
        content_result.is_err(),
        "same source membership with changed bytes must be RED"
    );
    fs::write(&baseline_path, base_source).unwrap();
    compile_fixture_rust(baseline_root.path(), base_source);
    let restored = scan_fixture_rust(baseline_root.path());
    assert_eq!(
        compare_production_origins(&fixture_inventory, &restored.production_origins),
        Ok(()),
        "restoring the source closure returns the candidate gate to baseline PASS"
    );
    eprintln!("RESTORED_JSON_ORIGIN_BASELINE_PASS");
}

#[test]
fn p1b_compiled_native_api_aliases_are_detected_and_macro_aliases_fail_closed() {
    let fixture = tempfile::tempdir().unwrap();
    let source = r#"
#![allow(non_snake_case)]
extern "C" { fn ShellExecuteExW(); }
mod fake_native_launcher {
    use crate::ShellExecuteExW as Launch;
    pub fn aliased_call() { unsafe { Launch() } }
    pub fn literal_call() { unsafe { crate::ShellExecuteExW() } }
    pub fn block_alias_call() { use crate::ShellExecuteExW as LocalLaunch; unsafe { LocalLaunch() } }
    macro_rules! aliased_macro { () => { unsafe { Launch() } } }
    pub fn macro_call() { aliased_macro!(); }
    #[cfg(test)] mod test_only {
        use crate::ShellExecuteExW as TestLaunch;
        pub fn excluded() { unsafe { TestLaunch() } }
    }
    #[cfg(not(test))] mod production_only {
        use crate::ShellExecuteExW as ProductionLaunch;
        pub fn included() { unsafe { ProductionLaunch() } }
    }
}
"#;
    compile_fixture_rust(fixture.path(), source);
    let scan = scan_fixture_rust(fixture.path());
    assert!(
        scan.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("enumerated native API macro tokens")),
        "macro use of an enumerated native API alias must fail closed: {:?}",
        scan.diagnostics
    );
    let aliases: BTreeSet<_> = ref_rows(&scan, "native-api::ShellExecuteExW")
        .iter()
        .map(|row| row.symbol.as_str())
        .collect();
    eprintln!("NATIVE_ALIAS_DETECTED\t{aliases:?}");
    eprintln!("NATIVE_MACRO_FAIL_CLOSED\t{:?}", scan.diagnostics);
    assert!(aliases.contains("crate::fake_native_launcher::aliased_call"));
    assert!(aliases.contains("crate::fake_native_launcher::literal_call"));
    assert!(aliases.contains("crate::fake_native_launcher::block_alias_call"));
    assert!(aliases.contains("crate::fake_native_launcher::production_only::included"));
    assert!(
        aliases.iter().all(|symbol| !symbol.contains("test_only::")),
        "cfg(test) alias callers must be excluded: {aliases:?}"
    );
    let raw_expected = accepted_raw_sites(&accepted_inventory());
    assert!(
        compare_raw(
            &raw_expected,
            &scan.sites.values().cloned().collect::<Vec<_>>()
        )
        .is_err(),
        "unknown fixture raw sites cannot be accepted silently"
    );
    assert!(
        compare_references(
            &accepted_references(&accepted_inventory()),
            &scan.references.values().cloned().collect::<Vec<_>>(),
        )
        .is_err(),
        "the actual alias calls must turn the exact accepted reference compare RED"
    );
}

#[test]
fn p1b_non_rust_release_asset_closure_matches_fingerprints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let actual = non_rust_fingerprints(&root).expect("non-Rust release asset closure must resolve");
    if env::var_os("WEBCODEX_P1B_DUMP_ASSETS").is_some() {
        for (path, digest) in &actual {
            eprintln!("P1B_ASSET\t{path}\t{digest}");
        }
    }
    let inventory = accepted_inventory();
    require_accepted_inventory(&inventory);
    assert_eq!(
        accepted_asset_fingerprints(&inventory), actual,
        "non-Rust source closure drift; review release/config roots and update the single reviewed JSON only"
    );
}

#[test]
fn p1b_non_rust_overlay_rejects_new_production_asset_but_ignores_test_only_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let temp = tempfile::tempdir().unwrap();
    let overlay = temp.path();
    for path in non_rust_release_assets(&root).unwrap() {
        let destination = overlay.join(&path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(root.join(&path), destination).unwrap();
    }
    let inventory = accepted_inventory();
    require_accepted_inventory(&inventory);
    let expected = accepted_asset_fingerprints(&inventory);
    let copied = non_rust_fingerprints(overlay).unwrap();
    assert_eq!(expected, copied, "accepted overlay matches JSON closure");

    let bin = overlay.join("npm/webcodex/bin");
    fs::create_dir_all(&bin).unwrap();
    for (name, source) in [
        ("p1b-overlay-launcher.mjs", "import { spawn } from 'node:child_process'; export const launch = () => spawn('tool', []);\n"),
        ("p1b-overlay-launcher.cjs", "const { spawn } = require('node:child_process'); exports.launch = () => spawn('tool', []);\n"),
        ("p1b-overlay-launcher", "#!/usr/bin/env node\nrequire('node:child_process').spawn('tool', []);\n"),
    ] {
        let fixture = bin.join(name);
        fs::write(&fixture, source).unwrap();
        let actual = non_rust_fingerprints(overlay).unwrap();
        assert!(actual.contains_key(&format!("npm/webcodex/bin/{name}")));
        assert_ne!(expected, actual, "asset compare must be RED for {name}");
        fs::remove_file(fixture).unwrap();
        assert_eq!(non_rust_fingerprints(overlay).unwrap(), expected);
    }

    fs::write(
        overlay.join("apps/desktop/src/unreferenced.test.tsx"),
        "import { spawn } from 'node:child_process';\n",
    )
    .unwrap();
    let test_only = non_rust_fingerprints(overlay).unwrap();
    assert_eq!(
        expected, test_only,
        "an unreferenced test asset stays outside the production closure"
    );

    fs::write(
        overlay.join("plugins/repo-info/src/p1b_new_production_launcher.ts"),
        "import { spawn } from 'node:child_process';\nexport const launch = () => spawn('tool', []);\n",
    )
    .unwrap();
    let entry = overlay.join("plugins/repo-info/src/plugin.ts");
    let mut entry_source = fs::read_to_string(&entry).unwrap();
    entry_source.push_str("\nimport './p1b_new_production_launcher';\n");
    fs::write(entry, entry_source).unwrap();
    let added = non_rust_fingerprints(overlay).unwrap();
    assert!(
        accepted_asset_fingerprints(&inventory) != added,
        "a new file selected by the production tsconfig must be RED"
    );
}

#[test]
#[ignore = "requires the external npm publication tool; run the explicit npm pack lane"]
fn p1b_npm_pack_and_asset_compare_reject_all_regular_bin_overlay_files() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let temp = tempfile::tempdir().unwrap();
    let overlay = temp.path();
    for path in non_rust_release_assets(&root).unwrap() {
        let destination = overlay.join(&path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(root.join(&path), destination).unwrap();
    }
    let inventory = accepted_inventory();
    let accepted = accepted_asset_fingerprints(&inventory);
    assert_eq!(non_rust_fingerprints(overlay).unwrap(), accepted);
    let package = overlay.join("npm/webcodex");
    let bin = package.join("bin");
    fs::create_dir_all(&bin).unwrap();

    for (name, source) in [
        ("p1b-overlay-launcher.mjs", "import { spawn } from 'node:child_process'; export const launch = () => spawn('tool', []);\n"),
        ("p1b-overlay-launcher.cjs", "const { spawn } = require('node:child_process'); exports.launch = () => spawn('tool', []);\n"),
        ("p1b-overlay-launcher", "#!/usr/bin/env node\nrequire('node:child_process').spawn('tool', []);\n"),
    ] {
        let fixture = bin.join(name);
        fs::write(&fixture, source).unwrap();
        let child = Command::new("npm")
            .args(["pack", "--dry-run", "--json", "--ignore-scripts", "--offline"])
            .current_dir(&package)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("npm must be installed for the publication closure negative control");
        let (status, stdout, stderr) = bounded_child_output(
            child,
            Instant::now() + Duration::from_secs(20),
            "npm pack publication closure fixture",
            64 * 1024,
            16 * 1024,
        )
        .expect("npm pack must complete within its deadline");
        assert!(status.success(), "npm pack failed: {}", String::from_utf8_lossy(&stderr));
        let output: serde_json::Value = serde_json::from_slice(&stdout).expect("npm pack JSON output");
        let packed = output[0]["files"].as_array().expect("npm pack files array");
        assert!(packed.iter().any(|row| row["path"] == format!("bin/{name}")), "npm must publish {name}");
        let actual = non_rust_fingerprints(overlay).unwrap();
        assert!(actual.contains_key(&format!("npm/webcodex/bin/{name}")), "regular file discovery must include {name}");
        assert_ne!(accepted, actual, "accepted asset compare must be RED for {name}");
        eprintln!("NPM_PACK_OFFLINE_IGNORE_SCRIPTS	bin/{name}	status=0	published=true");
        eprintln!("NPM_ASSET_COMPARE_RED	{}/bin/{name}", "npm/webcodex");
        fs::remove_file(fixture).unwrap();
        assert_eq!(non_rust_fingerprints(overlay).unwrap(), accepted, "restoring the overlay returns asset compare to baseline PASS");
        eprintln!("NPM_ASSET_RESTORE_PASS	{}/bin/{name}", "npm/webcodex");
    }
}

fn compare_raw(expected: &[RawSite], actual: &[RawSite]) -> Result<(), String> {
    let expected: BTreeSet<_> = expected.iter().cloned().collect();
    let actual: BTreeSet<_> = actual.iter().cloned().collect();
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "inventory drift; missing={:?}; added={:?}",
            expected.difference(&actual).collect::<Vec<_>>(),
            actual.difference(&expected).collect::<Vec<_>>()
        ))
    }
}

fn compare_references(
    expected: &[SymbolReference],
    actual: &[SymbolReference],
) -> Result<(), String> {
    let expected: BTreeSet<_> = expected.iter().cloned().collect();
    let actual: BTreeSet<_> = actual.iter().cloned().collect();
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "reference inventory drift; missing={:?}; added={:?}",
            expected.difference(&actual).collect::<Vec<_>>(),
            actual.difference(&expected).collect::<Vec<_>>()
        ))
    }
}

fn compare_scan(expected: &[RawSite], actual: &Scan) -> Result<(), String> {
    if !actual.diagnostics.is_empty() {
        return Err(format!(
            "scan diagnostics require review: {:?}",
            actual.diagnostics
        ));
    }
    compare_raw(
        expected,
        &actual.sites.values().cloned().collect::<Vec<_>>(),
    )
}

#[derive(Default)]
struct BoundaryBodyVisitor {
    file: String,
    impl_name: Option<String>,
    caller: String,
    bodies: BTreeMap<String, String>,
    references: BTreeMap<String, usize>,
}

impl BoundaryBodyVisitor {
    fn record_body(&mut self, name: &str, body: &syn::Block) {
        let qualified = self
            .impl_name
            .as_ref()
            .map(|implementation| format!("{implementation}::{name}"))
            .unwrap_or_else(|| name.to_owned());
        let key = format!("{}::{qualified}", self.file);
        if !required_boundary_body_keys().contains(&key.as_str()) {
            return;
        }
        let normalized = body.to_token_stream().to_string();
        let digest = Sha256::digest(normalized.as_bytes());
        self.bodies.insert(key, format!("{digest:x}"));
    }

    fn record_reference(&mut self, target: &str) {
        let key = format!("{}::{} -> {target}", self.file, self.caller);
        *self.references.entry(key).or_default() += 1;
    }
}

impl<'ast> Visit<'ast> for BoundaryBodyVisitor {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if attrs_possible(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !attrs_possible(&item.attrs) {
            return;
        }
        self.record_body(&item.sig.ident.to_string(), &item.block);
        let caller = self
            .impl_name
            .as_ref()
            .map(|implementation| format!("{implementation}::{}", item.sig.ident))
            .unwrap_or_else(|| item.sig.ident.to_string());
        let previous = std::mem::replace(&mut self.caller, caller);
        syn::visit::visit_item_fn(self, item);
        self.caller = previous;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !attrs_possible(&item.attrs) {
            return;
        }
        let previous = self
            .impl_name
            .replace(item.self_ty.to_token_stream().to_string());
        syn::visit::visit_item_impl(self, item);
        self.impl_name = previous;
    }

    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        if let syn::Stmt::Macro(item) = stmt {
            if !attrs_possible(&item.attrs) {
                return;
            }
        }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr_attrs(expr) {
            Some(attrs) if attrs_possible(attrs) => syn::visit::visit_expr(self, expr),
            Some(_) => {}
            None => {
                self.references.insert(
                    format!("{}::unsupported-expression-cfg", self.file),
                    usize::MAX,
                );
            }
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !attrs_possible(&item.attrs) {
            return;
        }
        self.record_body(&item.sig.ident.to_string(), &item.block);
        let caller = self
            .impl_name
            .as_ref()
            .map(|implementation| format!("{implementation}::{}", item.sig.ident))
            .unwrap_or_else(|| item.sig.ident.to_string());
        let previous = std::mem::replace(&mut self.caller, caller);
        syn::visit::visit_impl_item_fn(self, item);
        self.caller = previous;
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expr.func.as_ref() {
            if let Some(target) = path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            {
                if [
                    "configured_script_runtime_plan",
                    "capture_profile_env_snapshot",
                    "get_or_prepare",
                    "prepare",
                    "native_command",
                    "validation_module_available",
                    "validate_validation_steps",
                    "is_canonical",
                    "run_probe_output",
                    "developer_dir_via",
                ]
                .contains(&target.as_str())
                {
                    self.record_reference(&target);
                }
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let target = expr.method.to_string();
        if [
            "start_shell_job",
            "get_or_prepare",
            "prepare",
            "native_command",
            "is_canonical",
            "run_probe_output",
            "developer_dir_via",
        ]
        .contains(&target.as_str())
        {
            self.record_reference(&target);
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

#[test]
fn p1b_required_boundary_bodies_and_direct_callers_are_discoverable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let paths = [
        "crates/webcodex-runner/src/webcodex_runner/shell.rs",
        "crates/webcodex-runner/src/webcodex_runner/plugin.rs",
        "crates/webcodex-runner/src/webcodex_runner/job_manager.rs",
        "crates/webcodex-runner/src/main.rs",
        "crates/webcodex-core/src/runner_operation.rs",
        "crates/webcodex-core/src/runner_protocol/job.rs",
        "crates/webcodex-workspace/src/git_broker.rs",
    ];
    let mut bodies = BTreeMap::new();
    let mut references = BTreeMap::new();
    for relative in paths {
        let source = fs::read_to_string(root.join(relative)).unwrap();
        let syntax = syn::parse_file(&source).unwrap();
        let mut visitor = BoundaryBodyVisitor {
            file: relative.to_owned(),
            ..BoundaryBodyVisitor::default()
        };
        visitor.visit_file(&syntax);
        bodies.extend(visitor.bodies);
        references.extend(visitor.references);
    }
    for key in required_boundary_body_keys() {
        assert!(
            bodies.contains_key(*key),
            "missing required boundary body {key}"
        );
    }
    compare_boundary_references(
        &accepted_boundary_references(&accepted_inventory()),
        &references,
    )
    .unwrap_or_else(|error| {
        panic!("accepted direct shared-boundary caller origins drift: {error}")
    });
    assert_eq!(
        accepted_body_fingerprints(&accepted_inventory()),
        bodies,
        "accepted shared boundary body fingerprints drift"
    );
    if env::var_os("WEBCODEX_P1B_DUMP_BOUNDARIES").is_some() {
        for (symbol, digest) in bodies {
            eprintln!("P1B_BODY\t{symbol}\t{digest}");
        }
        for (reference, count) in references {
            eprintln!("P1B_BOUNDARY_REFERENCE\t{reference}\t{count}");
        }
    }
}

#[test]
fn p1b_boundary_reference_compare_rejects_new_and_renamed_callers() {
    let scan = |source: &str| {
        let syntax = syn::parse_file(source).unwrap();
        let mut visitor = BoundaryBodyVisitor {
            file: "crates/webcodex-runner/src/webcodex_runner/shell.rs".to_owned(),
            ..BoundaryBodyVisitor::default()
        };
        visitor.visit_file(&syntax);
        visitor.references
    };
    let baseline = scan("fn existing_caller() { prepare(); }\n");
    let expected = baseline;
    assert_eq!(compare_boundary_references(&expected, &expected), Ok(()));
    let added = scan(
        "fn existing_caller() { prepare(); }\nfn newly_added_caller() { native_command(); }\n",
    );
    let added_result = compare_boundary_references(&expected, &added);
    eprintln!("EXPECTED_NEW_BOUNDARY_CALLER_RED\t{added_result:?}");
    assert!(
        added_result.is_err(),
        "new direct caller must make exact compare RED"
    );

    let renamed = scan("fn renamed_caller() { prepare(); }\n");
    let renamed_result = compare_boundary_references(&expected, &renamed);
    eprintln!("EXPECTED_RENAMED_BOUNDARY_CALLER_RED\t{renamed_result:?}");
    assert!(
        renamed_result.is_err(),
        "renaming an existing caller must turn the exact compare RED"
    );
    let restored = scan("fn existing_caller() { prepare(); }\n");
    assert_eq!(compare_boundary_references(&expected, &restored), Ok(()));
    eprintln!("RESTORED_BOUNDARY_REFERENCE_BASELINE_PASS");
}

fn required_boundary_body_keys() -> &'static [&'static str] {
    &[
        "crates/webcodex-runner/src/webcodex_runner/shell.rs::configured_script_runtime_plan",
        "crates/webcodex-runner/src/webcodex_runner/shell.rs::capture_profile_env_snapshot",
        "crates/webcodex-runner/src/webcodex_runner/shell.rs::PreparedShellProfileCache::get_or_prepare",
        "crates/webcodex-runner/src/webcodex_runner/shell.rs::PreparedExecutionEnvironment::prepare",
        "crates/webcodex-runner/src/webcodex_runner/shell.rs::PreparedExecutionEnvironment::native_command",
        "crates/webcodex-runner/src/main.rs::validation_module_available",
        "crates/webcodex-core/src/runner_operation.rs::validate_validation_steps",
        "crates/webcodex-core/src/runner_protocol/job.rs::ShellJobValidationStep::is_canonical",
        "crates/webcodex-workspace/src/git_broker.rs::run_probe_output",
        "crates/webcodex-workspace/src/git_broker.rs::developer_dir_via",
    ]
}

#[test]
fn p1b_exact_inventory_compare_rejects_count_and_symbol_drift() {
    let site = RawSite {
        file: "src/a.rs".into(),
        symbol: "crate::launch".into(),
        primitive: "Command::new".into(),
        count: 1,
        targets: BTreeSet::from(["fixture:lib".into()]),
    };
    assert_eq!(compare_raw(&[site.clone()], &[site.clone()]), Ok(()));
    let mut count_drift = site.clone();
    count_drift.count += 1;
    assert!(compare_raw(&[site.clone()], &[count_drift]).is_err());
    let mut moved = site.clone();
    moved.symbol = "crate::new_module::launch".into();
    assert!(compare_raw(&[site], &[moved]).is_err());
}

#[test]
fn p1b_parser_follows_real_imports_and_production_cfg() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod launchers; #[cfg(test)] mod test_only; #[cfg(not(test))] mod production_only;",
    )
    .unwrap();
    fs::write(root.join("src/launchers.rs"), r#"use std::process::Command as C; pub fn launch() { let _ = C::new("x").spawn(); } pub fn local_alias() { use std::process::Command as LocalCmd; let _ = LocalCmd::new("y").status(); } pub fn brokered(broker: &Broker, spec: &Spec) { broker.spawn_with_toolchain(spec, &[]); } pub fn expression_cfg() { #[cfg(test)] { let _ = C::new("test-only-block").status(); } #[cfg(not(test))] { let _ = C::new("production-block").output(); } } #[cfg(test)] fn test_fn() { let _ = C::new("t").spawn(); }"#).unwrap();
    fs::write(
        root.join("src/test_only.rs"),
        r#"pub fn test_launcher() { let _ = std::process::Command::new("x").spawn(); }"#,
    )
    .unwrap();
    fs::write(
        root.join("src/production_only.rs"),
        r#"pub fn production_launcher() { let _ = std::process::Command::new("x").spawn(); }"#,
    )
    .unwrap();
    let mut scan = Scan::default();
    scan_file(
        root,
        &root.join("src/lib.rs"),
        &root.join("src"),
        "fixture:lib",
        "crate",
        &mut BTreeSet::new(),
        &mut scan,
    );
    assert!(
        scan.diagnostics.is_empty(),
        "overlay must resolve every production module: {:?}",
        scan.diagnostics
    );
    let raw: Vec<_> = scan.sites.values().cloned().collect();
    let symbols: BTreeSet<_> = scan
        .sites
        .values()
        .map(|site| site.symbol.as_str())
        .collect();
    assert!(
        symbols.contains("crate::launchers::launch"),
        "aliased production constructor must be found: {symbols:?}"
    );
    assert!(
        symbols.contains("crate::launchers::local_alias"),
        "block-local aliased constructor must be found: {symbols:?}"
    );
    assert!(
        scan.sites
            .values()
            .any(|site| site.symbol == "crate::launchers::brokered"
                && site.primitive == "method::spawn_with_toolchain"),
        "ExecutionBroker instance method must be discovered even when its implementation delegates to ManagedChild"
    );
    let expression_cfg_rows: Vec<_> = scan
        .sites
        .values()
        .filter(|site| site.symbol == "crate::launchers::expression_cfg")
        .collect();
    assert!(expression_cfg_rows
        .iter()
        .any(|site| site.primitive == "Command::new" && site.count == 1));
    assert!(expression_cfg_rows
        .iter()
        .any(|site| site.primitive == "method::output" && site.count == 1));
    assert!(
        expression_cfg_rows
            .iter()
            .all(|site| site.primitive != "method::status"),
        "the cfg(test) expression block must be absent: {expression_cfg_rows:?}"
    );
    assert!(
        symbols.contains("crate::production_only::production_launcher"),
        "cfg(not(test)) must be included: {symbols:?}"
    );
    assert!(
        symbols.iter().all(|symbol| !symbol.contains("test")),
        "cfg(test) module/function must be excluded: {symbols:?}"
    );
    assert!(
        compare_raw(&[], &raw).is_err(),
        "the real compare path must reject the newly added production launcher"
    );

    fs::write(root.join("src/launchers.rs"), r#"use std::process::Command as C; macro_rules! launch { () => { C::new("macro").output() } } pub fn macro_alias() { launch!(); }"#).unwrap();
    let mut macro_alias = Scan::default();
    scan_file(
        root,
        &root.join("src/lib.rs"),
        &root.join("src"),
        "fixture:lib",
        "crate",
        &mut BTreeSet::new(),
        &mut macro_alias,
    );
    assert!(
        macro_alias
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("unresolved process-like macro tokens")),
        "aliased std::process::Command macro launch must produce a fail-closed diagnostic: {:?}",
        macro_alias.diagnostics
    );
    assert!(
        compare_scan(&raw, &macro_alias).is_err(),
        "the production inventory compare must reject an unreviewed alias macro"
    );

    fs::write(root.join("src/launchers.rs"), r#"use std::process::Command as C; pub fn launch() { let _ = C::new("x").spawn(); let _ = C::new("y").status(); } pub fn local_alias() { use std::process::Command as LocalCmd; let _ = LocalCmd::new("y").status(); } #[cfg(test)] fn test_fn() { let _ = C::new("t").spawn(); }"#).unwrap();
    let mut count_drift = Scan::default();
    scan_file(
        root,
        &root.join("src/lib.rs"),
        &root.join("src"),
        "fixture:lib",
        "crate",
        &mut BTreeSet::new(),
        &mut count_drift,
    );
    assert!(
        compare_raw(
            &raw,
            &count_drift.sites.values().cloned().collect::<Vec<_>>()
        )
        .is_err(),
        "an extra primitive occurrence in the same allowed function must be RED through the real parser/compare path"
    );

    fs::write(root.join("src/lib.rs"), "mod launchers; mod fake_new_launcher; #[cfg(test)] mod test_only; #[cfg(not(test))] mod production_only;").unwrap();
    fs::write(root.join("src/fake_new_launcher.rs"), r#"pub fn launch() { use std::process::Command as Alias; let _ = Alias::new("new").spawn(); }"#).unwrap();
    let mut new_module = Scan::default();
    scan_file(
        root,
        &root.join("src/lib.rs"),
        &root.join("src"),
        "fixture:lib",
        "crate",
        &mut BTreeSet::new(),
        &mut new_module,
    );
    assert!(
        compare_raw(
            &raw,
            &new_module.sites.values().cloned().collect::<Vec<_>>()
        )
        .is_err(),
        "a newly aliased production module must be RED through the real parser/compare path"
    );
}
