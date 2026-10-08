//! Parse supported Rust constructs without treating comments or strings as code.
//! Parsing is syntactic; Pinocchio scans select test/no-entrypoint cfgs under package defaults.

use crate::input::{evidence, inside, read_text};
use anyhow::Result;
use solcompat_core::{Evidence, SourceSignal};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
};

#[derive(Default)]
struct Entrypoint(bool);
impl<'ast> Visit<'ast> for Entrypoint {
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if item.mac.path.segments.last().is_some_and(|s| {
            matches!(
                s.ident.to_string().as_str(),
                "entrypoint" | "program_entrypoint"
            )
        }) {
            self.0 = true;
        }
        visit::visit_item_macro(self, item);
    }
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if attribute.path().is_ident("program") {
            self.0 = true;
        }
        visit::visit_attribute(self, attribute);
    }
}

pub(crate) fn has_program_entrypoint(
    root: &Path,
    manifest: &Path,
    document: &toml::Value,
) -> Result<bool> {
    let directory = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cargo manifest has no parent: {}", manifest.display()))?;
    let library = document
        .get("lib")
        .and_then(|v| v.get("path"))
        .and_then(toml::Value::as_str)
        .map_or_else(|| directory.join("src/lib.rs"), |path| directory.join(path));
    let mut files = vec![(library.clone(), library.parent().unwrap().to_path_buf())];
    let mut seen = BTreeSet::new();
    while let Some((candidate, directory)) = files.pop() {
        if !candidate.is_file() {
            continue;
        }
        let candidate = inside(root, &candidate)?;
        if !seen.insert(candidate.clone()) {
            continue;
        }
        if let Ok(file) = syn::parse_file(&read_text(&candidate)?) {
            let mut entrypoint = Entrypoint::default();
            entrypoint.visit_file(&file);
            if entrypoint.0 {
                return Ok(true);
            }
            let mut modules = Modules {
                directory,
                paths: vec![],
                incomplete: false,
                purpose: ModulePurpose::DiscoverCandidate,
                profile: DefaultProfile::default(),
            };
            modules.visit_file(&file);
            files.extend(modules.paths);
        }
    }
    Ok(false)
}

fn conditional(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}

/// Bounded package-default, non-test profile. Other cfgs remain unresolved.
#[derive(Clone, Copy, Default)]
struct DefaultProfile {
    pinocchio: bool,
    no_entrypoint: Option<bool>,
}
impl DefaultProfile {
    fn from_manifest(document: &toml::Value, framework: &str) -> Self {
        let mut selected = BTreeSet::new();
        let mut pending = vec!["default".to_owned()];
        let features = document.get("features").and_then(toml::Value::as_table);
        let mut valid = document.get("features").is_none_or(|v| v.is_table());
        while let Some(name) = pending.pop() {
            if !selected.insert(name.clone()) {
                continue;
            }
            if let Some(value) = features.and_then(|f| f.get(&name)) {
                let Some(items) = value.as_array() else {
                    valid = false;
                    break;
                };
                for item in items {
                    let Some(feature) = item.as_str() else {
                        valid = false;
                        break;
                    };
                    // Dependency feature requests never enable a same-named local feature.
                    if !feature.contains('/') && !feature.starts_with("dep:") {
                        pending.push(feature.to_owned());
                    }
                }
            }
        }
        Self {
            pinocchio: framework == "pinocchio",
            no_entrypoint: valid.then(|| selected.contains("no-entrypoint")),
        }
    }
    fn predicate(self, meta: &syn::Meta) -> Option<bool> {
        match meta {
            syn::Meta::Path(path) if path.is_ident("test") => Some(false),
            syn::Meta::NameValue(value) if value.path.is_ident("feature") => {
                if let syn::Expr::Lit(lit) = &value.value {
                    if let syn::Lit::Str(name) = &lit.lit {
                        if name.value() == "no-entrypoint" {
                            return self.no_entrypoint;
                        }
                    }
                }
                None
            }
            syn::Meta::List(list) => {
                use syn::parse::Parser;
                let items =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                        .parse2(list.tokens.clone())
                        .ok()?;
                let values: Vec<_> = items.iter().map(|item| self.predicate(item)).collect();
                if list.path.is_ident("not") && values.len() == 1 {
                    values[0].map(|v| !v)
                } else if list.path.is_ident("all") {
                    if values.contains(&Some(false)) {
                        Some(false)
                    } else if values.contains(&None) {
                        None
                    } else {
                        Some(true)
                    }
                } else if list.path.is_ident("any") {
                    if values.contains(&Some(true)) {
                        Some(true)
                    } else if values.contains(&None) {
                        None
                    } else {
                        Some(false)
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn selection(self, attributes: &[syn::Attribute]) -> Option<bool> {
        if !self.pinocchio {
            return if conditional(attributes) {
                None
            } else {
                Some(true)
            };
        }
        let mut unknown = false;
        for attr in attributes {
            if attr.path().is_ident("cfg_attr") {
                unknown = true;
            }
            if attr.path().is_ident("cfg") {
                match attr
                    .parse_args::<syn::Meta>()
                    .ok()
                    .and_then(|m| self.predicate(&m))
                {
                    Some(false) => return Some(false),
                    Some(true) => {}
                    None => unknown = true,
                }
            }
        }
        if unknown {
            None
        } else {
            Some(true)
        }
    }
}

fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// Discovery can identify a candidate in conditionally declared code without
/// claiming that a build selects it. Source checks apply the bounded default profile.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModulePurpose {
    DiscoverCandidate,
    SelectedSource,
}

/// Collect declared default-path modules under the requested evidence policy.
struct Modules {
    directory: PathBuf,
    paths: Vec<(PathBuf, PathBuf)>,
    incomplete: bool,
    purpose: ModulePurpose,
    profile: DefaultProfile,
}
impl<'ast> Visit<'ast> for Modules {
    // Only attributes selecting the file or a module affect module reachability.
    // A conditional import/macro must not discard unrelated unconditional functions.
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}
    fn visit_file(&mut self, file: &'ast syn::File) {
        if self.purpose == ModulePurpose::SelectedSource {
            match self.profile.selection(&file.attrs) {
                Some(false) => return,
                None => {
                    self.incomplete = true;
                    return;
                }
                Some(true) => {}
            }
        }
        visit::visit_file(self, file);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if self.purpose == ModulePurpose::SelectedSource {
            match self.profile.selection(&item.attrs) {
                Some(false) => return,
                None => {
                    self.incomplete = true;
                    return;
                }
                Some(true) => {}
            }
        }
        if item.attrs.iter().any(|attr| attr.path().is_ident("path")) {
            self.incomplete = true;
            return;
        }
        let directory = self.directory.join(item.ident.to_string());
        if let Some((_, items)) = &item.content {
            let previous = std::mem::replace(&mut self.directory, directory);
            for item in items {
                self.visit_item(item);
            }
            self.directory = previous;
        } else {
            let flat = directory.with_extension("rs");
            let nested = directory.join("mod.rs");
            let path = match (flat.is_file(), nested.is_file()) {
                (true, false) => flat,
                (false, true) => nested,
                _ => {
                    self.incomplete = true;
                    return;
                }
            };
            self.paths.push((path, directory));
        }
    }
}

struct Signals<'a> {
    root: &'a Path,
    path: &'a Path,
    source: &'a str,
    framework: &'a str,
    profile: DefaultProfile,
    signals: BTreeMap<SourceSignal, Vec<Evidence>>,
    incomplete: bool,
}
impl Signals<'_> {
    /// Preserve uncertainty only when a conditional item contains a reviewed signal.
    fn skip_conditional(
        &mut self,
        attributes: &[syn::Attribute],
        scan: impl FnOnce(&mut Self),
    ) -> bool {
        match self.profile.selection(attributes) {
            Some(true) => return false,
            Some(false) => return true,
            None => {}
        }
        let mut candidate = Self {
            root: self.root,
            path: self.path,
            source: self.source,
            framework: self.framework,
            profile: self.profile,
            signals: BTreeMap::new(),
            incomplete: false,
        };
        scan(&mut candidate);
        self.incomplete |= !candidate.signals.is_empty() || candidate.incomplete;
        true
    }

    fn record(&mut self, signal: SourceSignal, span: proc_macro2::Span) {
        let mut ev = evidence(
            self.root,
            self.path,
            &format!("/source:{}", span.start().line.max(1)),
            signal.evidence_kind(),
            self.source.as_bytes(),
        );
        ev.kind = signal.evidence_kind().into();
        let items = self.signals.entry(signal).or_default();
        if !items.iter().any(|item| item.pointer == ev.pointer) {
            items.push(ev);
        }
    }
    fn qualified(&mut self, parts: &[String], span: proc_macro2::Span) {
        if self.framework != "pinocchio" || parts.first().is_none_or(|p| p != "pinocchio") {
            return;
        }
        if parts.ends_with(&["account_info".into(), "AccountInfo".into()]) {
            self.record(SourceSignal::PinocchioAccountInfo, span);
        }
        if parts.ends_with(&["pubkey".into(), "Pubkey".into()]) {
            self.record(SourceSignal::PinocchioPubkey, span);
        }
    }
    fn use_tree(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_tree(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => {
                prefix.push(name.ident.to_string());
                self.qualified(prefix, name.span());
                prefix.pop();
            }
            syn::UseTree::Rename(rename) => {
                prefix.push(rename.ident.to_string());
                self.qualified(prefix, rename.span());
                prefix.pop();
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.use_tree(item, prefix);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }
}
impl<'ast> Visit<'ast> for Signals<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        match self.profile.selection(&file.attrs) {
            Some(false) => return,
            None => {
                self.incomplete = true;
                return;
            }
            Some(true) => {}
        }
        visit::visit_file(self, file);
    }
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if self.skip_conditional(item_attributes(item), |candidate| {
            visit::visit_item(candidate, item)
        }) {
            return;
        }
        visit::visit_item(self, item);
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if self.skip_conditional(&item.attrs, |candidate| {
            visit::visit_impl_item_fn(candidate, item)
        }) {
            return;
        }
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.use_tree(&item.tree, &mut Vec::new());
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.qualified(
            &path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>(),
            path.span(),
        );
        visit::visit_path(self, path);
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if self.framework == "pinocchio" && item.sig.ident == "process_instruction" {
            for argument in item.sig.inputs.iter().skip(1).take(1) {
                let syn::FnArg::Typed(argument) = argument else {
                    continue;
                };
                let syn::Type::Reference(reference) = &*argument.ty else {
                    continue;
                };
                let syn::Type::Slice(slice) = &*reference.elem else {
                    continue;
                };
                let syn::Type::Path(path) = &*slice.elem else {
                    continue;
                };
                if path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "AccountView")
                {
                    self.record(
                        if reference.mutability.is_some() {
                            SourceSignal::PinocchioMutableEntrypoint
                        } else {
                            SourceSignal::PinocchioImmutableEntrypoint
                        },
                        item.sig.span(),
                    );
                }
            }
        }
        visit::visit_item_fn(self, item);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.skip_conditional(&call.attrs, |candidate| {
            visit::visit_expr_call(candidate, call)
        }) {
            return;
        }
        if self.framework == "anchor" {
            if let syn::Expr::Path(path) = &*call.func {
                let parts: Vec<_> = path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                if parts.len() >= 2
                    && parts.last().is_some_and(|p| p == "discriminator")
                    && call.args.is_empty()
                {
                    self.record(SourceSignal::AnchorDiscriminatorMethod, call.span());
                }
                if parts.len() >= 2
                    && parts[parts.len() - 2] == "CpiContext"
                    && parts
                        .last()
                        .is_some_and(|p| matches!(p.as_str(), "new" | "new_with_signer"))
                {
                    if let Some(syn::Expr::MethodCall(first)) = call.args.first() {
                        if first.method == "to_account_info" && first.args.is_empty() {
                            self.record(SourceSignal::AnchorCpiContextAccountInfo, call.span());
                        }
                    }
                }
            }
        }
        visit::visit_expr_call(self, call);
    }
}

#[derive(Default)]
pub(crate) struct SourceObservations {
    pub(crate) signals: BTreeMap<SourceSignal, Vec<Evidence>>,
    pub(crate) incomplete: Vec<Evidence>,
}

pub(crate) fn program_source_signals(
    root: &Path,
    manifest: &Path,
    framework: &str,
    document: &toml::Value,
) -> Result<SourceObservations> {
    let directory = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cargo manifest has no parent"))?;
    let library = directory.join(
        document
            .get("lib")
            .and_then(|v| v.get("path"))
            .and_then(toml::Value::as_str)
            .unwrap_or("src/lib.rs"),
    );
    let mut all = BTreeMap::<SourceSignal, Vec<Evidence>>::new();
    let mut incomplete = Vec::new();
    if !library.is_file() {
        return Ok(SourceObservations {
            signals: all,
            incomplete,
        });
    }
    let mut files = vec![(library.clone(), library.parent().unwrap().to_path_buf())];
    let mut seen = BTreeSet::new();
    while let Some((path, directory)) = files.pop() {
        let path = inside(root, &path)?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let source = read_text(&path)?;
        let unresolved = || evidence(root, &path, "/source", "unresolved-rust", source.as_bytes());
        let Ok(file) = syn::parse_file(&source) else {
            incomplete.push(unresolved());
            continue;
        };
        let mut modules = Modules {
            directory,
            paths: vec![],
            incomplete: false,
            purpose: ModulePurpose::SelectedSource,
            profile: DefaultProfile::from_manifest(document, framework),
        };
        modules.visit_file(&file);
        if modules.incomplete {
            incomplete.push(unresolved());
        }
        files.extend(modules.paths);
        let mut visitor = Signals {
            root,
            path: &path,
            source: &source,
            framework,
            profile: DefaultProfile::from_manifest(document, framework),
            signals: BTreeMap::new(),
            incomplete: false,
        };
        visitor.visit_file(&file);
        if visitor.incomplete && !modules.incomplete {
            incomplete.push(unresolved());
        }
        for (signal, observations) in visitor.signals {
            all.entry(signal).or_default().extend(observations);
        }
    }
    incomplete.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(SourceObservations {
        signals: all,
        incomplete,
    })
}
