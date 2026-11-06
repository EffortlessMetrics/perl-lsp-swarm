//! Bounded source-qualified storage joins; unsupported provenance fails closed.
use super::{CheckResult, ExpressionMatch, MatchesInput, model, test_only, type_name};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use syn::visit::{self, Visit};

#[derive(Clone, Default)]
struct Scope {
    module: Vec<String>,
    imports: BTreeMap<String, Vec<String>>,
    shadowed: BTreeSet<String>,
    generic_types: BTreeSet<String>,
    unresolved_local_types: bool,
}

impl Scope {
    fn with_generics(mut self, generics: &syn::Generics) -> Self {
        self.generic_types
            .extend(generics.type_params().map(|parameter| parameter.ident.to_string()));
        self
    }

    fn path(&self, parts: &[String]) -> Vec<String> {
        let Some(first) = parts.first() else { return Vec::new() };
        if self.shadowed.contains(first) {
            return self.module.iter().cloned().chain(parts.iter().cloned()).collect();
        }
        if let Some(import) = self.imports.get(first) {
            return import.iter().cloned().chain(parts.iter().skip(1).cloned()).collect();
        }
        let mut base = self.module.clone();
        let mut rest = parts;
        if first == "crate" {
            base.truncate(1);
            rest = parts.get(1..).unwrap_or_default();
        } else if first == "self" {
            rest = parts.get(1..).unwrap_or_default();
        } else if first == "super" {
            while rest.first().is_some_and(|part| part == "super") {
                base.pop();
                rest = rest.get(1..).unwrap_or_default();
            }
        } else if matches!(
            first.as_str(),
            "std" | "core" | "alloc" | "parking_lot" | "perl_lsp_rs" | "perl_lsp_rs_core"
        ) {
            return parts.to_vec();
        } else if parts.len() == 1 {
            // The actual Rust prelude, unless a source import shadows it.
            let prelude = match first.as_str() {
                "Option" => Some("std::option::Option"),
                "Result" => Some("std::result::Result"),
                "Vec" => Some("std::vec::Vec"),
                _ => None,
            };
            if let Some(path) = prelude {
                return path.split("::").map(str::to_string).collect();
            }
        }
        base.extend_from_slice(rest);
        base
    }

    fn with_imports(mut self, items: &[syn::Item]) -> CheckResult<Self> {
        fn entries(
            tree: &syn::UseTree,
            prefix: Vec<String>,
            result: &mut Vec<(String, Vec<String>)>,
        ) {
            match tree {
                syn::UseTree::Path(path) => {
                    let mut prefix = prefix;
                    prefix.push(path.ident.to_string());
                    entries(&path.tree, prefix, result);
                }
                syn::UseTree::Name(name) => {
                    let mut path = prefix;
                    if name.ident != "self" {
                        path.push(name.ident.to_string());
                    }
                    if let Some(alias) = path.last() {
                        result.push((alias.clone(), path.clone()));
                    }
                }
                syn::UseTree::Rename(rename) => {
                    let mut path = prefix;
                    path.push(rename.ident.to_string());
                    result.push((rename.rename.to_string(), path));
                }
                syn::UseTree::Group(group) => {
                    for item in &group.items {
                        entries(item, prefix.clone(), result);
                    }
                }
                // Glob imports cannot anchor a bare storage type.
                syn::UseTree::Glob(_) => {}
            }
        }
        for item in items {
            let name = match item {
                syn::Item::Mod(item) => Some(&item.ident),
                syn::Item::Struct(item) => Some(&item.ident),
                syn::Item::Enum(item) => Some(&item.ident),
                syn::Item::Type(item) => Some(&item.ident),
                syn::Item::Trait(item) => Some(&item.ident),
                syn::Item::Union(item) => Some(&item.ident),
                _ => None,
            };
            if let Some(name) = name {
                self.shadowed.insert(name.to_string());
            }
        }
        let mut imports = Vec::new();
        for item in items {
            if let syn::Item::Use(import) = item
                && !test_only(&import.attrs)
            {
                entries(&import.tree, Vec::new(), &mut imports);
            }
        }
        for (alias, path) in imports {
            let qualified = self.path(&path);
            if self
                .imports
                .insert(alias.clone(), qualified.clone())
                .is_some_and(|previous| previous != qualified)
            {
                return Err(format!("ambiguous storage import {alias}").into());
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Container {
    Arc,
    LazyLock,
    ParkingMutex,
    StdMutex,
    ParkingRwLock,
    StdRwLock,
    Option,
    Result,
    Vec,
    Iterator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StorageType {
    Named(String),
    Container(Container, Box<StorageType>),
}

impl StorageType {
    fn dereferenced(&self) -> &Self {
        match self {
            Self::Container(Container::Arc | Container::LazyLock, inner) => inner.dereferenced(),
            _ => self,
        }
    }
    fn named(&self) -> Option<&str> {
        match self.dereferenced() {
            Self::Named(name) => Some(name),
            _ => None,
        }
    }
}

struct Structure {
    fields: BTreeMap<String, syn::Type>,
    scope: Scope,
    derives_clone: bool,
}
struct Function {
    owner: Option<String>,
    signature: syn::Signature,
    body: syn::Block,
    scope: Scope,
}
type Functions = BTreeMap<(String, String), Vec<Function>>;

#[derive(Default)]
pub(super) struct StorageAnchors {
    structures: BTreeMap<String, Structure>,
    statics: BTreeMap<String, (syn::Type, Scope)>,
    functions: Functions,
    inherent_methods: BTreeSet<(String, String)>,
    reexports: BTreeMap<String, Vec<String>>,
}

fn source_module(path: &str) -> Vec<String> {
    if let Some((package, relative)) =
        path.strip_prefix("crates/").and_then(|path| path.split_once("/src/"))
    {
        let mut module = vec![package.replace('-', "_")];
        let relative = relative.strip_suffix(".rs").unwrap_or(relative);
        module.extend(
            relative
                .split('/')
                .filter(|part| !matches!(*part, "mod" | "lib" | "main"))
                .map(str::to_string),
        );
        module
    } else {
        vec![path.trim_end_matches(".rs").replace('/', "::")]
    }
}

impl StorageAnchors {
    #[cfg(test)]
    pub(super) fn remove_declaration(&mut self, id: &str) {
        self.structures.remove(id);
    }
    pub(super) fn add_items(
        &mut self,
        path: &str,
        prefix: &str,
        items: &[syn::Item],
    ) -> CheckResult {
        let mut module = source_module(path);
        module.extend(
            prefix
                .trim_end_matches("::")
                .split("::")
                .filter(|part| !part.is_empty())
                .map(str::to_string),
        );
        self.collect(path, prefix, Scope { module, ..Scope::default() }, items)
    }

    fn collect(
        &mut self,
        path: &str,
        prefix: &str,
        scope: Scope,
        items: &[syn::Item],
    ) -> CheckResult {
        let scope = scope.with_imports(items)?;
        for item in items {
            if let syn::Item::Use(import) = item
                && matches!(import.vis, syn::Visibility::Public(_))
                && !test_only(&import.attrs)
            {
                fn public_globs(
                    tree: &syn::UseTree,
                    prefix: Vec<String>,
                    paths: &mut Vec<Vec<String>>,
                ) {
                    match tree {
                        syn::UseTree::Path(path) => {
                            let mut prefix = prefix;
                            prefix.push(path.ident.to_string());
                            public_globs(&path.tree, prefix, paths);
                        }
                        syn::UseTree::Group(group) => {
                            for child in &group.items {
                                public_globs(child, prefix.clone(), paths);
                            }
                        }
                        syn::UseTree::Glob(_) => paths.push(prefix),
                        _ => {}
                    }
                }
                let mut paths = Vec::new();
                public_globs(&import.tree, Vec::new(), &mut paths);
                for path in paths {
                    self.reexports
                        .entry(scope.module.join("::"))
                        .or_default()
                        .push(scope.path(&path).join("::"));
                }
            }
        }
        for item in items {
            match item {
                syn::Item::Struct(record) if !test_only(&record.attrs) => {
                    let id = scope
                        .module
                        .iter()
                        .cloned()
                        .chain([record.ident.to_string()])
                        .collect::<Vec<_>>()
                        .join("::");
                    let derives_clone = record.attrs.iter().filter(|attr| attr.path().is_ident("derive")).any(|attr| {
                        attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
                            .is_ok_and(|paths| paths.iter().any(|path| path.is_ident("Clone")))
                    }) && !scope.imports.contains_key("Clone");
                    let fields = record
                        .fields
                        .iter()
                        .filter_map(|field| {
                            field.ident.as_ref().map(|name| (name.to_string(), field.ty.clone()))
                        })
                        .collect();
                    if self
                        .structures
                        .insert(
                            id.clone(),
                            Structure { fields, scope: scope.clone(), derives_clone },
                        )
                        .is_some()
                    {
                        return Err(format!("ambiguous storage declaration {id}").into());
                    }
                }
                syn::Item::Static(value) if !test_only(&value.attrs) => {
                    let id = scope.path(&[value.ident.to_string()]).join("::");
                    if self
                        .statics
                        .insert(id.clone(), ((*value.ty).clone(), scope.clone()))
                        .is_some()
                    {
                        return Err(format!("ambiguous storage static {id}").into());
                    }
                }
                syn::Item::Fn(function) if !test_only(&function.attrs) => {
                    self.functions
                        .entry((path.into(), format!("{prefix}{}", function.sig.ident)))
                        .or_default()
                        .push(Function {
                            owner: None,
                            signature: function.sig.clone(),
                            body: (*function.block).clone(),
                            scope: scope.clone().with_generics(&function.sig.generics),
                        });
                }
                syn::Item::Impl(implementation) if !test_only(&implementation.attrs) => {
                    let Some(owner_spelling) = type_name(&implementation.self_ty) else { continue };
                    let owner = scope
                        .path(&owner_spelling.split("::").map(str::to_string).collect::<Vec<_>>())
                        .join("::");
                    let trait_prefix = implementation
                        .trait_
                        .as_ref()
                        .map(|(name, _)| {
                            format!(
                                "{}::",
                                name.segments
                                    .iter()
                                    .map(|s| s.ident.to_string())
                                    .collect::<Vec<_>>()
                                    .join("::")
                            )
                        })
                        .unwrap_or_default();
                    for item in &implementation.items {
                        if let syn::ImplItem::Fn(function) = item
                            && !test_only(&function.attrs)
                        {
                            if implementation.trait_.is_none() {
                                self.inherent_methods
                                    .insert((owner.clone(), function.sig.ident.to_string()));
                            }
                            self.functions
                                .entry((
                                    path.into(),
                                    format!(
                                        "{prefix}{owner_spelling}::{trait_prefix}{}",
                                        function.sig.ident
                                    ),
                                ))
                                .or_default()
                                .push(Function {
                                    owner: Some(owner.clone()),
                                    signature: function.sig.clone(),
                                    body: function.block.clone(),
                                    scope: scope
                                        .clone()
                                        .with_generics(&implementation.generics)
                                        .with_generics(&function.sig.generics),
                                });
                        }
                    }
                }
                syn::Item::Mod(module) if !test_only(&module.attrs) => {
                    if let Some((_, items)) = &module.content {
                        let mut child = scope.module.clone();
                        child.push(module.ident.to_string());
                        self.collect(
                            path,
                            &format!("{prefix}{}::", module.ident),
                            Scope { module: child, ..Scope::default() },
                            items,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn from_projection(
        root: &Path,
        projection: &model::Projection,
    ) -> CheckResult<Self> {
        let mut anchors = Self::default();
        let mut paths: BTreeSet<_> = projection
            .witnesses
            .values()
            .filter(|w| w.path.ends_with(".rs"))
            .map(|w| w.path.as_str())
            .collect();
        // These source-carried public re-exports anchor the actual server fields.
        paths.extend([
            "crates/perl-lsp-rs/src/state/mod.rs",
            "crates/perl-lsp-rs/src/state/config.rs",
            "crates/perl-lsp-rs/src/runtime/workspace_folder.rs",
        ]);
        for path in paths {
            anchors.add_items(
                path,
                "",
                &syn::parse_file(&fs::read_to_string(root.join(path))?)?.items,
            )?;
        }
        Ok(anchors)
    }

    fn resolve_type(&self, ty: &syn::Type, scope: &Scope) -> Option<StorageType> {
        if scope.unresolved_local_types {
            return None;
        }
        match ty {
            syn::Type::Reference(reference) => self.resolve_type(&reference.elem, scope),
            syn::Type::Path(path) if path.qself.is_none() => {
                let parts: Vec<_> =
                    path.path.segments.iter().map(|s| s.ident.to_string()).collect();
                if path.path.leading_colon.is_none()
                    && parts.first().is_some_and(|first| scope.generic_types.contains(first))
                {
                    return None;
                }
                let local = scope
                    .module
                    .iter()
                    .cloned()
                    .chain(parts.iter().cloned())
                    .collect::<Vec<_>>()
                    .join("::");
                let id = if path.path.leading_colon.is_some() {
                    parts.join("::")
                } else if parts.len() == 1 && self.structures.contains_key(&local) {
                    local
                } else {
                    scope.path(&parts).join("::")
                };
                if let Some(id) = self.declaration(&id, &mut BTreeSet::new()) {
                    return Some(StorageType::Named(id));
                }
                let container = match id.as_str() {
                    "std::sync::Arc" => Container::Arc,
                    "std::sync::LazyLock" => Container::LazyLock,
                    "std::sync::Mutex" => Container::StdMutex,
                    "std::sync::RwLock" => Container::StdRwLock,
                    "parking_lot::Mutex" => Container::ParkingMutex,
                    "parking_lot::RwLock" => Container::ParkingRwLock,
                    "std::option::Option" => Container::Option,
                    "std::result::Result" => Container::Result,
                    "std::vec::Vec" => Container::Vec,
                    _ => return None,
                };
                let segment = path.path.segments.last()?;
                let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return None;
                };
                let syn::GenericArgument::Type(inner) = arguments.args.first()? else {
                    return None;
                };
                Some(StorageType::Container(container, Box::new(self.resolve_type(inner, scope)?)))
            }
            _ => None,
        }
    }

    fn declaration(&self, id: &str, seen: &mut BTreeSet<String>) -> Option<String> {
        if self.structures.contains_key(id) {
            return Some(id.into());
        }
        if !seen.insert(id.into()) {
            return None;
        }
        let (module, name) = id.rsplit_once("::")?;
        let mut found = BTreeSet::new();
        for target in self.reexports.get(module)? {
            if let Some(id) = self.declaration(&format!("{target}::{name}"), &mut seen.clone()) {
                found.insert(id);
            }
        }
        if found.len() == 1 { found.into_iter().next() } else { None }
    }

    fn canonical_owner(&self, owner: &str, member: &str) -> CheckResult<String> {
        let spelling =
            if owner == "Limits" { "LspLimits".into() } else { format!("{owner}Config") };
        let candidates: Vec<_> = self
            .structures
            .iter()
            .filter(|(id, _)| id.rsplit("::").next() == Some(&spelling))
            .collect();
        let [(id, structure)] = candidates.as_slice() else {
            return Err("unresolved or ambiguous canonical owner declaration".into());
        };
        if structure.fields.contains_key(member) {
            return Ok((*id).clone());
        }
        let nested: Vec<_> = structure
            .fields
            .values()
            .filter_map(|ty| self.resolve_type(ty, &structure.scope))
            .filter_map(|ty| ty.named().map(str::to_string))
            .filter(|id| self.structures.get(id).is_some_and(|s| s.fields.contains_key(member)))
            .collect();
        match nested.as_slice() {
            [owner] => Ok(owner.clone()),
            _ => Err("unresolved or ambiguous canonical storage member".into()),
        }
    }
}

#[derive(Clone)]
struct QualifiedUse<'a> {
    anchors: &'a StorageAnchors,
    scope: Scope,
    locals: BTreeMap<String, Option<StorageType>>,
    self_owner: Option<String>,
    expected_owner: &'a str,
    member: &'a str,
    target: &'a syn::Expr,
    inside: bool,
    writes: bool,
    reads: bool,
}

impl QualifiedUse<'_> {
    fn owner(&self, expression: &syn::Expr) -> Option<StorageType> {
        match expression {
            syn::Expr::Path(path) => {
                let names: Vec<_> =
                    path.path.segments.iter().map(|s| s.ident.to_string()).collect();
                if let [name] = names.as_slice() {
                    if name == "self" {
                        return self
                            .self_owner
                            .clone()
                            .filter(|id| self.anchors.structures.contains_key(id))
                            .map(StorageType::Named);
                    }
                    if let Some(owner) = self.locals.get(name) {
                        return owner.clone();
                    }
                }
                let (ty, scope) = self.anchors.statics.get(&self.scope.path(&names).join("::"))?;
                self.anchors.resolve_type(ty, scope)
            }
            syn::Expr::Field(field) => {
                let owner = self.owner(&field.base)?;
                let structure = self.anchors.structures.get(owner.named()?)?;
                let syn::Member::Named(member) = &field.member else { return None };
                self.anchors
                    .resolve_type(structure.fields.get(&member.to_string())?, &structure.scope)
            }
            syn::Expr::Reference(reference) => self.owner(&reference.expr),
            syn::Expr::Paren(paren) => self.owner(&paren.expr),
            syn::Expr::Group(group) => self.owner(&group.expr),
            syn::Expr::MethodCall(call) if call.method == "find" && call.args.len() == 1 => {
                let receiver = self.owner(&call.receiver)?;
                match receiver.dereferenced() {
                    StorageType::Container(Container::Iterator, element) => {
                        Some(StorageType::Container(Container::Option, element.clone()))
                    }
                    _ => None,
                }
            }
            syn::Expr::MethodCall(call) if call.args.is_empty() => {
                let owner = self.owner(&call.receiver)?;
                if let Some(name) = owner.named()
                    && self
                        .anchors
                        .inherent_methods
                        .contains(&(name.into(), call.method.to_string()))
                {
                    return None;
                }
                match (call.method.to_string().as_str(), owner.dereferenced()) {
                    ("lock", StorageType::Container(Container::ParkingMutex, inner)) => {
                        Some((**inner).clone())
                    }
                    ("lock", StorageType::Container(Container::StdMutex, inner)) => {
                        Some(StorageType::Container(Container::Result, inner.clone()))
                    }
                    ("read", StorageType::Container(Container::StdRwLock, inner)) => {
                        Some(StorageType::Container(Container::Result, inner.clone()))
                    }
                    ("read", StorageType::Container(Container::ParkingRwLock, inner)) => {
                        Some((**inner).clone())
                    }
                    ("clone", StorageType::Named(name))
                        if self.anchors.structures.get(name).is_some_and(|s| s.derives_clone) =>
                    {
                        Some(owner)
                    }
                    ("clone", StorageType::Container(Container::Arc, _)) => Some(owner),
                    ("iter" | "iter_mut", StorageType::Container(Container::Vec, element)) => {
                        Some(StorageType::Container(Container::Iterator, element.clone()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_storage(&self, field: &syn::ExprField) -> bool {
        matches!(&field.member, syn::Member::Named(member) if member == self.member)
            && self.owner(&field.base).as_ref().and_then(StorageType::named)
                == Some(self.expected_owner)
    }

    fn bind(&mut self, pattern: &syn::Pat, owner: Option<StorageType>) {
        match pattern {
            syn::Pat::Ident(ident) => {
                self.locals.insert(ident.ident.to_string(), owner);
                if let Some((_, nested)) = &ident.subpat {
                    self.bind(nested, None);
                }
            }
            syn::Pat::Type(typed) => {
                let declared = self.anchors.resolve_type(&typed.ty, &self.scope);
                self.bind(&typed.pat, owner.filter(|owner| Some(owner) == declared.as_ref()));
            }
            syn::Pat::TupleStruct(pattern)
                if pattern.path.is_ident("Some")
                    && !self.scope.imports.contains_key("Some")
                    && pattern.elems.len() == 1 =>
            {
                let inner = match owner {
                    Some(StorageType::Container(Container::Option, inner)) => Some(*inner),
                    _ => None,
                };
                if let Some(pattern) = pattern.elems.first() {
                    self.bind(pattern, inner);
                }
            }
            _ => {
                struct Names(Vec<String>);
                impl<'ast> Visit<'ast> for Names {
                    fn visit_pat_ident(&mut self, ident: &'ast syn::PatIdent) {
                        self.0.push(ident.ident.to_string());
                        visit::visit_pat_ident(self, ident);
                    }
                }
                let mut names = Names(Vec::new());
                names.visit_pat(pattern);
                for name in names.0 {
                    self.locals.insert(name, None);
                }
            }
        }
    }

    fn bind_initializer(&mut self, pattern: &syn::Pat, initializer: &syn::Expr) {
        match pattern {
            syn::Pat::Tuple(tuple) => {
                for (index, pattern) in tuple.elems.iter().enumerate() {
                    let owner = self.tuple_owner(initializer, index);
                    self.bind(pattern, owner);
                }
            }
            // Annotated tuple shapes are unsupported; annotations are not evidence.
            syn::Pat::Type(typed) if matches!(&*typed.pat, syn::Pat::Tuple(_)) => {
                self.bind(&typed.pat, None)
            }
            _ => {
                let owner = self.owner(initializer);
                self.bind(pattern, owner);
            }
        }
    }

    fn tuple_owner(&self, expression: &syn::Expr, index: usize) -> Option<StorageType> {
        match expression {
            syn::Expr::Tuple(tuple) => self.owner(tuple.elems.iter().nth(index)?),
            syn::Expr::Block(block) => self.tuple_block(&block.block, index),
            syn::Expr::Paren(paren) => self.tuple_owner(&paren.expr, index),
            syn::Expr::If(branch) => {
                let mut selected = self.clone();
                if let syn::Expr::Let(binding) = &*branch.cond {
                    let owner = self.owner(&binding.expr);
                    selected.bind(&binding.pat, owner);
                }
                let then_owner = selected.tuple_block(&branch.then_branch, index)?;
                let else_owner = self.tuple_owner(&branch.else_branch.as_ref()?.1, index)?;
                (then_owner == else_owner).then_some(then_owner)
            }
            _ => None,
        }
    }

    fn tuple_block(&self, block: &syn::Block, index: usize) -> Option<StorageType> {
        let mut local = self.clone();
        for (position, statement) in block.stmts.iter().enumerate() {
            match statement {
                syn::Stmt::Local(binding) if !test_only(&binding.attrs) => {
                    if let Some(initializer) = &binding.init {
                        local.bind_initializer(&binding.pat, &initializer.expr);
                    } else {
                        local.bind(&binding.pat, None);
                    }
                }
                syn::Stmt::Expr(expression, None)
                    if position.checked_add(1) == Some(block.stmts.len()) =>
                {
                    return local.tuple_owner(expression, index);
                }
                // Assignment/control flow before the result needs stronger analysis.
                syn::Stmt::Expr(_, _) | syn::Stmt::Macro(_) => return None,
                _ => {}
            }
        }
        None
    }
}

impl<'ast> Visit<'ast> for QualifiedUse<'_> {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let outer = self.locals.clone();
        let outer_scope = self.scope.clone();
        if block.stmts.iter().any(|statement| matches!(statement, syn::Stmt::Item(_))) {
            self.scope.unresolved_local_types = true;
        }
        visit::visit_block(self, block);
        self.locals = outer;
        self.scope = outer_scope;
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if test_only(&local.attrs) {
            return;
        }
        if let Some(initializer) = &local.init {
            self.visit_expr(&initializer.expr);
            self.bind_initializer(&local.pat, &initializer.expr);
        } else {
            self.bind(&local.pat, None);
        }
    }
    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        let previous = self.inside;
        self.inside |= expression == self.target;
        visit::visit_expr(self, expression);
        self.inside = previous;
    }
    fn visit_expr_let(&mut self, binding: &'ast syn::ExprLet) {
        self.visit_expr(&binding.expr);
        self.bind(&binding.pat, None);
    }
    fn visit_expr_if(&mut self, branch: &'ast syn::ExprIf) {
        let outer = self.locals.clone();
        self.visit_expr(&branch.cond);
        self.visit_block(&branch.then_branch);
        self.locals = outer.clone();
        if let Some((_, alternative)) = &branch.else_branch {
            self.visit_expr(alternative);
        }
        self.locals = outer;
    }
    fn visit_expr_while(&mut self, iteration: &'ast syn::ExprWhile) {
        let outer = self.locals.clone();
        self.visit_expr(&iteration.cond);
        self.visit_block(&iteration.body);
        self.locals = outer;
    }
    fn visit_expr_for_loop(&mut self, iteration: &'ast syn::ExprForLoop) {
        let outer = self.locals.clone();
        self.visit_expr(&iteration.expr);
        self.bind(&iteration.pat, None);
        self.visit_block(&iteration.body);
        self.locals = outer;
    }
    fn visit_expr_match(&mut self, branching: &'ast syn::ExprMatch) {
        self.visit_expr(&branching.expr);
        let outer = self.locals.clone();
        for arm in &branching.arms {
            if test_only(&arm.attrs) {
                continue;
            }
            self.locals = outer.clone();
            self.bind(&arm.pat, None);
            self.visit_pat(&arm.pat);
            self.visit_expr(&arm.body);
        }
        self.locals = outer;
    }
    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if self.inside
            && let syn::Expr::Field(field) = &*assignment.left
        {
            self.writes |= self.is_storage(field);
        }
        self.visit_expr(&assignment.right);
        if let syn::Expr::Path(path) = &*assignment.left
            && let Some(name) = path.path.get_ident()
        {
            self.locals.insert(name.to_string(), None);
        }
    }
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if self.inside {
            self.reads |= self.is_storage(field);
        }
        visit::visit_expr_field(self, field);
    }
    fn visit_expr_struct(&mut self, record: &'ast syn::ExprStruct) {
        let owner = if record.path.is_ident("Self") {
            self.self_owner.clone()
        } else {
            Some(
                self.scope
                    .path(
                        &record
                            .path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect::<Vec<_>>(),
                    )
                    .join("::"),
            )
        };
        if self.inside && owner.as_deref() == Some(self.expected_owner) {
            self.writes |= record.fields.iter().any(|field| matches!(&field.member, syn::Member::Named(member) if member == self.member));
        }
        visit::visit_expr_struct(self, record);
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.visit_expr(&call.receiver);
        for argument in &call.args {
            if call.method == "map"
                && let syn::Expr::Closure(closure) = argument
            {
                let outer = self.locals.clone();
                let receiver = self.owner(&call.receiver);
                let input = match receiver.as_ref().map(StorageType::dereferenced) {
                    Some(StorageType::Container(Container::Option | Container::Result, inner)) => {
                        Some((**inner).clone())
                    }
                    _ => None,
                };
                for pattern in &closure.inputs {
                    self.bind(pattern, input.clone());
                }
                self.visit_expr(&closure.body);
                self.locals = outer;
            } else {
                self.visit_expr(argument);
            }
        }
    }
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        let outer = self.locals.clone();
        for input in &closure.inputs {
            self.bind(input, None);
        }
        self.visit_expr(&closure.body);
        self.locals = outer;
    }
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if node.mac.path.is_ident("matches")
            && let Ok(arguments) = syn::parse2::<MatchesInput>(node.mac.tokens.clone())
        {
            // Guard pattern provenance is outside the supported grammar.
            if arguments.guard.is_some() {
                return;
            }
            self.visit_expr(&arguments.expression);
        }
    }
}

pub(super) fn check_storage_join(
    row: &model::Row,
    projection: &model::Projection,
    anchors: &StorageAnchors,
) -> CheckResult {
    if row.kind != "active" {
        return Ok(());
    }
    let (owner, member) =
        row.rust_field.split_once('.').ok_or("invalid canonical storage identity")?;
    let owner = anchors.canonical_owner(owner, member)?;
    let (mut has_write, mut has_read) = (false, false);
    for (references, writer) in [(&row.writers, true), (&row.consumers, false)] {
        for reference in references {
            let witness = projection.witnesses.get(reference).ok_or("unknown joined witness")?;
            if witness.path.ends_with(".ts") || witness.function.starts_with('@') {
                continue;
            }
            let expression: syn::Expr = syn::parse_str(&witness.expression)?;
            let contexts = anchors
                .functions
                .get(&(witness.path.clone(), witness.function.clone()))
                .ok_or("unresolved storage witness function")?;
            let matching: Vec<_> = contexts
                .iter()
                .filter(|context| {
                    let mut matcher = ExpressionMatch { expected: &expression, count: 0 };
                    if matches!(&expression, syn::Expr::Block(block) if block.block == context.body)
                    {
                        matcher.count += 1;
                    }
                    matcher.visit_block(&context.body);
                    matcher.count == 1
                })
                .collect();
            let [context] = matching.as_slice() else {
                return Err(format!(
                    "{}: unresolved or ambiguous storage witness {reference}",
                    row.id
                )
                .into());
            };
            let mut usage = QualifiedUse {
                anchors,
                scope: context.scope.clone(),
                locals: BTreeMap::new(),
                self_owner: context.owner.clone().filter(|owner| {
                    !owner
                        .rsplit("::")
                        .next()
                        .is_some_and(|name| context.scope.generic_types.contains(name))
                }),
                expected_owner: &owner,
                member,
                target: &expression,
                inside: matches!(&expression, syn::Expr::Block(block) if block.block == context.body),
                writes: false,
                reads: false,
            };
            for input in &context.signature.inputs {
                if let syn::FnArg::Typed(parameter) = input {
                    usage.bind(&parameter.pat, anchors.resolve_type(&parameter.ty, &context.scope));
                }
            }
            usage.visit_block(&context.body);
            if writer {
                has_write |= usage.writes;
            } else {
                has_read |= usage.reads;
            }
        }
    }
    if !has_write || (!row.consumers.is_empty() && !has_read) {
        return Err(format!(
            "{}: witness does not join canonical storage to writer/consumer",
            row.id
        )
        .into());
    }
    Ok(())
}
