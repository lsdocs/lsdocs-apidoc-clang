//! Headers to API items, with libclang.
//!
//! Each header is parsed on its own (a translation unit), and what it
//! declares itself (not what it includes) is documented: a module per
//! header, its namespaces, records, enums, functions, variables, type
//! aliases and macros.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use clang::diagnostic::Severity as ClangSeverity;
use clang::{Accessibility, Entity, EntityKind, Index};
use lsdocs_apidoc::{Item, ItemKind, Location, Signature, Visibility};
use lsdocs_plugin_sdk::protocol::{Diagnostic, Severity};
use serde::Deserialize;

use crate::Lang;
use crate::text::{self, Stop};

/// libclang allows one `Clang` at a time in a process.
static CLANG: Mutex<()> = Mutex::new(());

/// libclang, kept loaded until the process ends: `Clang` unloads it when
/// dropped, and LLVM's exit handlers then crash the process (seen on
/// Windows).
static LOADED: OnceLock<Arc<clang_sys::SharedLibrary>> = OnceLock::new();

/// How to install libclang, for the error when it cannot be loaded.
const INSTALL: &str = "apidoc-clang needs libclang (10 or later): `winget install LLVM.LLVM` (Windows), \
`xcode-select --install` or `brew install llvm` (macOS), `apt install libclang-dev` (Debian, Ubuntu), \
`dnf install clang-devel` (Fedora), `pacman -S clang` (Arch); set LIBCLANG_PATH to the directory \
holding it if it is elsewhere";

/// One entry of the `paths` option.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Entry {
    /// A header, or a directory of headers.
    Path(String),
    /// A header and its module's name, or a directory and its modules'
    /// prefix.
    Named {
        /// The path.
        path: String,
        /// The module name.
        module: String,
    },
}

/// What extraction found.
#[derive(Debug, Default)]
pub(crate) struct Found {
    pub items: Vec<Item>,
    pub files: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A header to document.
struct Header {
    /// Its path on disk.
    full: PathBuf,
    /// Its path from the site directory, `/`-separated.
    shown: String,
    /// Its module's name.
    module: String,
    /// How it is included: its path in the directory given.
    include: String,
}

/// Documents the headers at `entries` (relative to `root`), parsed with
/// `args`.
pub(crate) fn extract(
    entries: &[Entry],
    root: &Path,
    lang: Lang,
    args: &[String],
) -> Result<Found, String> {
    let _guard = CLANG.lock().unwrap_or_else(PoisonError::into_inner);
    let clang = clang::Clang::new().map_err(|e| format!("cannot load libclang: {e}. {INSTALL}"))?;
    if let Some(library) = clang_sys::get_library() {
        let _ = LOADED.set(library);
    }
    let index = Index::new(&clang, false, false);
    let mut found = Found::default();
    let mut headers: Vec<Header> = Vec::new();
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let (path, name) = match entry {
            Entry::Path(p) => (p.as_str(), None),
            Entry::Named { path, module } => (path.as_str(), Some(module.as_str())),
        };
        let full = root.join(path);
        if full.is_dir() {
            include_dirs.push(full.clone());
            let mut files = Vec::new();
            walk(&full, "", lang, &mut files);
            files.sort();
            for rel in files {
                let stem = rel.rsplit_once('.').map_or(rel.as_str(), |(s, _)| s);
                headers.push(Header {
                    full: full.join(&rel),
                    shown: format!("{}/{rel}", path.trim_end_matches(['/', '\\'])),
                    module: match name {
                        Some(prefix) => format!("{prefix}/{stem}"),
                        None => stem.to_owned(),
                    },
                    include: rel.clone(),
                });
            }
        } else if full.is_file() {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
            headers.push(Header {
                full,
                shown: path.replace('\\', "/"),
                module: name.unwrap_or(stem).to_owned(),
                include: file.to_owned(),
            });
        } else {
            found
                .diagnostics
                .push(problem(format!("{path}: no such file or directory")));
        }
    }
    let mut arguments: Vec<String> = match lang {
        Lang::C => vec!["-x".into(), "c".into(), "-std=c17".into()],
        Lang::Cpp => vec!["-x".into(), "c++".into(), "-std=c++20".into()],
    };
    arguments.extend(include_dirs.iter().map(|d| format!("-I{}", d.display())));
    arguments.extend(args.iter().cloned());
    for header in headers {
        let source = match std::fs::read_to_string(&header.full) {
            Ok(s) => s,
            Err(e) => {
                found
                    .diagnostics
                    .push(problem(format!("{}: {e}", header.shown)));
                continue;
            }
        };
        found.files.push(header.shown.clone());
        let tu = match index
            .parser(&header.full)
            .arguments(&arguments)
            .detailed_preprocessing_record(true)
            .skip_function_bodies(true)
            .keep_going(true)
            .parse()
        {
            Ok(tu) => tu,
            Err(e) => {
                found.diagnostics.push(problem(format!(
                    "{}: libclang cannot parse it: {e}",
                    header.shown
                )));
                continue;
            }
        };
        for d in tu.get_diagnostics() {
            if matches!(
                d.get_severity(),
                ClangSeverity::Error | ClangSeverity::Fatal
            ) {
                let location = d.get_location().get_file_location();
                let line = location.line;
                let file = location
                    .file
                    .map(|f| f.get_path())
                    .filter(|p| *p != header.full)
                    .map_or_else(|| header.shown.clone(), |p| p.display().to_string());
                found
                    .diagnostics
                    .push(problem(format!("{file}:{line}: {}", d.get_text())));
            }
        }
        let mut walker = Walker {
            lang,
            shown: &header.shown,
            source: &source,
            seen: HashSet::new(),
        };
        let mut module = Item::new(ItemKind::Module, vec![header.module.clone()]);
        module.signature = Some(text_signature(format!("#include \"{}\"", header.include)));
        module.docs = text::file_comment(&source).map(|d| walker.docs(&d));
        module.location = Some(Location {
            file: header.shown.clone(),
            line: Some(1),
        });
        module.members = walker.members(&tu.get_entity(), &module.path, false);
        found.items.push(module);
    }
    Ok(found)
}

/// Header extensions, by language.
fn is_header(name: &str, lang: Lang) -> bool {
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    match lang {
        Lang::C => ext == "h",
        Lang::Cpp => matches!(ext, "h" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "tpp"),
    }
}

fn walk(dir: &Path, prefix: &str, lang: Lang, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            walk(&path, &rel, lang, out);
        } else if is_header(&name, lang) {
            out.push(rel);
        }
    }
}

fn problem(message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        message,
        code: Some("apidoc-clang".into()),
        ..Diagnostic::default()
    }
}

fn text_signature(text: String) -> Signature {
    Signature {
        text: Some(text),
        ..Signature::default()
    }
}

struct Walker<'a> {
    lang: Lang,
    shown: &'a str,
    source: &'a str,
    /// Entities documented already (by USR): redeclarations are not.
    seen: HashSet<String>,
}

impl Walker<'_> {
    /// A comment's text as Markdown.
    fn docs(&self, stripped: &str) -> String {
        text::markdown(stripped, self.lang.domain(), self.lang.code_language())
    }

    fn comment(&self, entity: &Entity) -> Option<String> {
        let raw = entity.get_comment()?;
        let docs = self.docs(&text::strip(&raw));
        (!docs.trim().is_empty()).then_some(docs)
    }

    fn location(&self, entity: &Entity) -> Option<Location> {
        let location = entity.get_location()?.get_file_location();
        Some(Location {
            file: self.shown.to_owned(),
            line: Some(location.line),
        })
    }

    /// The documented entities inside `parent` (in the header itself);
    /// `in_record` when `parent` is a class, struct or union.
    fn members(&mut self, parent: &Entity, path: &[String], in_record: bool) -> Vec<Item> {
        let mut out: Vec<Item> = Vec::new();
        for child in parent.get_children() {
            if !child.is_in_main_file() {
                continue;
            }
            match child.get_kind() {
                EntityKind::LinkageSpec => {
                    let inner = self.members(&child, path, in_record);
                    for item in inner {
                        push(&mut out, item);
                    }
                }
                EntityKind::Namespace => {
                    let Some(name) = child.get_name() else {
                        continue; // Anonymous: internal linkage.
                    };
                    let mut full = path.to_vec();
                    full.push(name.clone());
                    let members = self.members(&child, &full, false);
                    // A namespace reopened in the header is one item.
                    if let Some(existing) = out
                        .iter_mut()
                        .find(|i| i.kind == ItemKind::Namespace && i.name() == name)
                    {
                        for member in members {
                            push(&mut existing.members, member);
                        }
                        continue;
                    }
                    let mut item = Item::new(ItemKind::Namespace, full);
                    item.signature = Some(text_signature(format!("namespace {name}")));
                    item.docs = self.comment(&child);
                    item.location = self.location(&child);
                    if matches!(name.as_str(), "detail" | "details" | "internal" | "impl") {
                        item.visibility = Visibility::Private;
                    }
                    item.members = members;
                    out.push(item);
                }
                EntityKind::ClassDecl
                | EntityKind::StructDecl
                | EntityKind::UnionDecl
                | EntityKind::ClassTemplate => {
                    if child.is_anonymous_record_decl() {
                        // C11's anonymous members: their fields are the
                        // parent's.
                        let inner = self.members(&child, path, true);
                        for item in inner {
                            push(&mut out, item);
                        }
                        continue;
                    }
                    if let Some(item) = self.record(&child, path, None) {
                        push(&mut out, item);
                    }
                }
                EntityKind::EnumDecl => {
                    if anonymous_enum(&child) {
                        // `enum { A, B };`: its enumerators are constants
                        // of the scope.
                        for item in self.enumerators(&child, path, ItemKind::Constant) {
                            push(&mut out, item);
                        }
                        continue;
                    }
                    if let Some(item) = self.enumeration(&child, path, None) {
                        push(&mut out, item);
                    }
                }
                EntityKind::FunctionDecl
                | EntityKind::FunctionTemplate
                | EntityKind::Method
                | EntityKind::Constructor
                | EntityKind::Destructor
                | EntityKind::ConversionFunction => {
                    if let Some(item) = self.function(&child, path, in_record) {
                        push(&mut out, item);
                    }
                }
                EntityKind::FieldDecl | EntityKind::VarDecl => {
                    if let Some(item) = self.variable(&child, path, in_record) {
                        push(&mut out, item);
                    }
                }
                EntityKind::TypedefDecl
                | EntityKind::TypeAliasDecl
                | EntityKind::TypeAliasTemplateDecl => {
                    if let Some(item) = self.alias(&child, path) {
                        push(&mut out, item);
                    }
                }
                EntityKind::MacroDefinition => {
                    if let Some(item) = self.macro_item(&child, path) {
                        push(&mut out, item);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Whether the entity was documented already (a redeclaration).
    fn seen(&mut self, entity: &Entity) -> bool {
        match entity.get_usr() {
            Some(usr) => !self.seen.insert(usr.0),
            None => false,
        }
    }

    /// The common parts of an item.
    fn item(
        &self,
        entity: &Entity,
        kind: ItemKind,
        full: Vec<String>,
        sig: Option<String>,
    ) -> Item {
        let mut item = Item::new(kind, full);
        item.signature = sig.map(text_signature);
        item.docs = self.comment(entity);
        item.location = self.location(entity);
        item.visibility = match entity.get_accessibility() {
            Some(Accessibility::Private) => Visibility::Private,
            Some(Accessibility::Protected) => Visibility::Protected,
            _ => Visibility::Public,
        };
        if item.docs.as_deref().is_some_and(|d| {
            d.lines().any(|l| {
                let t = l.trim_start();
                ["@internal", "\\internal", "@private", "\\private"]
                    .iter()
                    .any(|tag| t.starts_with(tag))
            })
        }) {
            item.visibility = Visibility::Private;
        }
        item
    }

    /// A class, struct or union (named `name`, for a typedef of an
    /// anonymous one).
    fn record(&mut self, entity: &Entity, path: &[String], name: Option<&str>) -> Option<Item> {
        // Specializations document as their template.
        if entity.get_template().is_some()
            || entity.get_kind() == EntityKind::ClassTemplatePartialSpecialization
        {
            return None;
        }
        if !entity.is_definition() {
            // A declaration: documented if it is all there is (an opaque
            // type) and it says something.
            let has_definition = entity.get_definition().is_some();
            if has_definition || entity.get_comment().is_none() {
                return None;
            }
        }
        // Anonymous ones are documented by the typedef naming them.
        if name.is_none() && unnamed(entity) {
            return None;
        }
        let name = name.map(str::to_owned).or_else(|| entity.get_name())?;
        if self.seen(entity) {
            return None;
        }
        let kind = match entity.get_kind() {
            EntityKind::ClassTemplate => {
                entity.get_template_kind().unwrap_or(EntityKind::ClassDecl)
            }
            other => other,
        };
        let kind = match kind {
            EntityKind::StructDecl => ItemKind::Struct,
            EntityKind::UnionDecl => ItemKind::Union,
            _ => ItemKind::Class,
        };
        let mut full = path.to_vec();
        full.push(name);
        let sig = text::declaration(entity, Stop::Body);
        let mut item = self.item(entity, kind, full.clone(), sig);
        item.bases = entity
            .get_children()
            .iter()
            .filter(|c| c.get_kind() == EntityKind::BaseSpecifier)
            .filter_map(Entity::get_display_name)
            .collect();
        if entity.is_definition() {
            item.members = self.members(entity, &full, true);
        }
        Some(item)
    }

    fn enumeration(
        &mut self,
        entity: &Entity,
        path: &[String],
        name: Option<&str>,
    ) -> Option<Item> {
        if !entity.is_definition() || name.is_none() && unnamed(entity) {
            return None;
        }
        let name = name.map(str::to_owned).or_else(|| entity.get_name())?;
        let mut full = path.to_vec();
        full.push(name);
        let sig = text::declaration(entity, Stop::Body);
        let mut item = self.item(entity, ItemKind::Enum, full.clone(), sig);
        // An unscoped enum's enumerators are also names of its scope;
        // documented under the enum.
        item.members = self.enumerators(entity, &full, ItemKind::Variant);
        Some(item)
    }

    fn enumerators(&mut self, entity: &Entity, path: &[String], kind: ItemKind) -> Vec<Item> {
        entity
            .get_children()
            .iter()
            .filter(|c| c.get_kind() == EntityKind::EnumConstantDecl)
            .filter_map(|c| {
                let name = c.get_name()?;
                let mut full = path.to_vec();
                full.push(name);
                let sig = text::declaration(c, Stop::Whole);
                let mut item = self.item(c, kind.clone(), full, sig);
                item.value = c.get_enum_constant_value().map(|(v, _)| v.to_string());
                Some(item)
            })
            .collect()
    }

    fn function(&mut self, entity: &Entity, path: &[String], in_record: bool) -> Option<Item> {
        let name = entity.get_name()?;
        if self.seen(entity) {
            return None;
        }
        let kind = match entity.get_kind() {
            EntityKind::Constructor => ItemKind::Constructor,
            EntityKind::FunctionDecl => ItemKind::Function,
            EntityKind::FunctionTemplate if !in_record => ItemKind::Function,
            EntityKind::Method | EntityKind::FunctionTemplate if entity.is_static_method() => {
                ItemKind::Function
            }
            _ => ItemKind::Method,
        };
        let mut full = path.to_vec();
        full.push(name);
        let sig = text::declaration(entity, Stop::Function);
        let mut item = self.item(entity, kind, full, sig);
        // An override says what it overrides, unless it says more.
        if item.docs.is_none() {
            item.docs = entity
                .get_overridden_methods()
                .into_iter()
                .flatten()
                .find_map(|m| self.comment(&m));
        }
        Some(item)
    }

    fn variable(&mut self, entity: &Entity, path: &[String], in_record: bool) -> Option<Item> {
        let name = entity.get_name()?;
        if self.seen(entity) {
            return None;
        }
        let ty = entity.get_type();
        let constant = ty.is_some_and(|t| t.is_const_qualified())
            || entity
                .get_range()
                .is_some_and(|r| r.tokenize().iter().any(|t| t.get_spelling() == "constexpr"));
        let kind = match entity.get_kind() {
            EntityKind::FieldDecl => ItemKind::Field,
            _ if constant => ItemKind::Constant,
            _ if in_record => ItemKind::Field,
            _ => ItemKind::Variable,
        };
        let mut full = path.to_vec();
        full.push(name);
        let sig = text::declaration(entity, Stop::Whole).map(|s| text::cut(&s));
        let mut item = self.item(entity, kind, full, sig);
        item.ty = ty.map(|t| t.get_display_name());
        Some(item)
    }

    /// A typedef or alias; a typedef of an anonymous record or enum is
    /// that record or enum, named after the typedef.
    fn alias(&mut self, entity: &Entity, path: &[String]) -> Option<Item> {
        let name = entity.get_name()?;
        if self.seen(entity) {
            return None;
        }
        let declared = entity
            .get_typedef_underlying_type()
            .and_then(|t| t.get_declaration());
        if let Some(declared) = declared
            && declared.is_in_main_file()
            && matches!(
                declared.get_kind(),
                EntityKind::StructDecl | EntityKind::UnionDecl | EntityKind::EnumDecl
            )
        {
            // `typedef struct Point Point;`: the struct is documented.
            let anonymous = unnamed(&declared);
            // `typedef struct point point;`: the struct is documented (if
            // it is defined; else the typedef is all there is).
            if !anonymous
                && declared.get_name().as_deref() == Some(name.as_str())
                && declared.get_definition().is_some()
            {
                return None;
            }
            if anonymous {
                let mut item = if declared.get_kind() == EntityKind::EnumDecl {
                    self.enumeration(&declared, path, Some(&name))?
                } else {
                    self.record(&declared, path, Some(&name))?
                };
                item.signature = text::declaration(entity, Stop::Whole).map(text_signature);
                if item.docs.is_none() {
                    item.docs = self.comment(entity);
                }
                item.location = self.location(entity);
                return Some(item);
            }
        }
        let mut full = path.to_vec();
        full.push(name);
        let sig = text::declaration(entity, Stop::Whole);
        let mut item = self.item(entity, ItemKind::TypeAlias, full, sig);
        item.ty = entity
            .get_typedef_underlying_type()
            .map(|t| t.get_display_name());
        Some(item)
    }

    fn macro_item(&mut self, entity: &Entity, path: &[String]) -> Option<Item> {
        if entity.is_builtin_macro() {
            return None;
        }
        let name = entity.get_name()?;
        let sig = text::macro_definition(entity)?;
        // Include guards: `#define NAME_H` with nothing after it.
        if !entity.is_function_like_macro()
            && sig.split_whitespace().count() == 2
            && ["_H", "_H_", "_HPP", "_HPP_", "_INCLUDED", "_HXX"]
                .iter()
                .any(|s| name.ends_with(s))
        {
            return None;
        }
        let line = entity.get_location()?.get_file_location().line;
        let mut full = path.to_vec();
        full.push(name);
        let mut item = self.item(entity, ItemKind::Macro, full, Some(sig));
        if item.docs.is_none() {
            item.docs = text::comment_above(self.source, line)
                .map(|c| self.docs(&text::strip(&c)))
                .filter(|d| !d.trim().is_empty());
        }
        Some(item)
    }
}

/// Whether a record or enum has no name of its own: anonymous, or named
/// only by a typedef (`typedef struct { ... } point;`, which clang names
/// `point`).
fn unnamed(entity: &Entity) -> bool {
    if entity.is_anonymous() || entity.get_name().is_none() {
        return true;
    }
    typedef_named(entity)
}

/// Whether an enum has no name at all (`enum { A, B };`), not even a
/// typedef's: clang names it `enum (unnamed at ...)`.
fn anonymous_enum(entity: &Entity) -> bool {
    entity
        .get_name()
        .is_none_or(|n| n.contains("(unnamed") || n.contains("(anonymous"))
}

/// Whether a record or enum is written without a name (`struct {`).
fn typedef_named(entity: &Entity) -> bool {
    let Some(range) = entity.get_range() else {
        return false;
    };
    let tokens = range.tokenize();
    let mut words = tokens.iter().map(|t| t.get_spelling());
    matches!(
        words.next().as_deref(),
        Some("struct" | "union" | "enum" | "class")
    ) && words.next().as_deref() == Some("{")
}

/// Adds an item to its scope's; overloads (callables of one name) are one
/// item, with each signature, documented by the first that says
/// something.
fn push(items: &mut Vec<Item>, item: Item) {
    if item.kind.is_callable()
        && let Some(existing) = items
            .iter_mut()
            .find(|i| i.kind.is_callable() && i.name() == item.name())
    {
        if let (Some(sig), Some(new)) = (
            existing.signature.as_mut().and_then(|s| s.text.as_mut()),
            item.signature.as_ref().and_then(|s| s.text.as_ref()),
        ) {
            sig.push('\n');
            sig.push_str(new);
        }
        if existing.docs.is_none() {
            existing.docs = item.docs;
        }
        return;
    }
    items.push(item);
}
