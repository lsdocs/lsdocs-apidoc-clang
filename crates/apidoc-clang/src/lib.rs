//! First-party plugins documenting C and C++ APIs from their headers, with
//! libclang.
//!
//! libclang is loaded when the plugin runs, not linked: building lsdocs
//! needs no LLVM, using these plugins needs libclang 10 or later
//! installed (see the README). Headers are parsed as a compiler would
//! (with the `args` given: include directories, defines, the standard),
//! so types and templates are clang's; nothing is compiled or run.
//!
//! Two plugins, one per language, each with its domain as Sphinx has it:
//!
//! ```toml
//! [[plugins]]
//! name = "cpp"
//! builtin = "apidoc-cpp"
//! options = { paths = ["include/mylib"], args = ["-DMYLIB_API="] }
//! ```
//!
//! - **Paths** (relative to the site directory): a header is a module,
//!   named after its file; a directory every header below it (named by
//!   their paths in it), and an include directory (`-I`) for them all.
//!   `{ path, module }` names a header's module (or prefixes a
//!   directory's).
//! - **What is documented:** what each header declares itself:
//!   namespaces, classes, structs and unions with their members (fields,
//!   methods, constructors, nested types), enums and their enumerators,
//!   functions and function templates, variables and constants, type
//!   aliases and typedefs (a typedef of an anonymous struct is that
//!   struct), and macros (include guards aside). Private and protected
//!   members are private or protected; so are `detail` namespaces and
//!   comments tagged `@internal`. Overloads are one item with each
//!   signature.
//! - **Doc comments** are Doxygen's (`/** */`, `///`, `//!`, `///<`):
//!   Markdown with `@param` / `\param`, `@return`, `@throws`, `@tparam`,
//!   `@deprecated` and the like as sections, `\code` blocks, `\c`, `\p`,
//!   `\b`, `\e` markup, and `\ref name` / `#name` references. A header's
//!   `@file` comment documents its module.
//! - **The domains:** `c` (`c:function`, `c:struct`, `c:union`,
//!   `c:enum`, `c:enumerator`, `c:member`, `c:var`, `c:macro`,
//!   `c:type`; roles `c:func`, `c:struct`, `c:member`, `c:data`,
//!   `c:macro`, `c:type`, ...) and `cpp` (`cpp:class`, `cpp:struct`,
//!   `cpp:function`, `cpp:member`, `cpp:var`, `cpp:type`, `cpp:enum`,
//!   `cpp:enumerator`, ...; roles `cpp:class`, `cpp:func`, `cpp:member`,
//!   `cpp:type`, `cpp:any`, ...), with the framework's `module`,
//!   `currentmodule` and `auto`.
//!
//! Options: `paths` (required for generated pages), `args` (compiler
//! arguments, after the defaults `-x c -std=c17` or `-x c++
//! -std=c++20`), and those of the framework (`output`, `private`,
//! `title`, `doc_format`, `doc_style`, `generate`, `primary`).

mod extract;
#[cfg(test)]
mod tests;
mod text;

use std::path::Path;

use lsdocs_apidoc::index::split_name;
use lsdocs_apidoc::{DocStyle, Item, ItemKind, Signature};
use lsdocs_apidoc_plugin::{Extracted, Language};
use lsdocs_plugin_sdk::Plugin;
use serde_json::Value;

/// The C plugin (`apidoc-c`), as registered with the host.
pub fn plugin_c() -> Plugin {
    lsdocs_apidoc_plugin::plugin(CFamily { lang: Lang::C }, env!("CARGO_PKG_VERSION"))
}

/// The C++ plugin (`apidoc-cpp`), as registered with the host.
pub fn plugin_cpp() -> Plugin {
    lsdocs_apidoc_plugin::plugin(CFamily { lang: Lang::Cpp }, env!("CARGO_PKG_VERSION"))
}

/// A language of the C family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// C.
    C,
    /// C++.
    Cpp,
}

impl Lang {
    fn domain(self) -> &'static str {
        match self {
            Lang::C => "c",
            Lang::Cpp => "cpp",
        }
    }

    fn code_language(self) -> &'static str {
        match self {
            Lang::C => "c",
            Lang::Cpp => "cpp",
        }
    }
}

/// C or C++, through libclang.
#[derive(Debug, Clone, Copy)]
pub struct CFamily {
    /// The language.
    pub lang: Lang,
}

impl Language for CFamily {
    fn domain(&self) -> &str {
        self.lang.domain()
    }

    fn name(&self) -> &str {
        match self.lang {
            Lang::C => "C",
            Lang::Cpp => "C++",
        }
    }

    fn code_language(&self) -> &str {
        self.lang.code_language()
    }

    fn separator(&self) -> &str {
        match self.lang {
            Lang::C => ".",
            Lang::Cpp => "::",
        }
    }

    fn settings(&self) -> &[&str] {
        &["paths", "args"]
    }

    fn doc_format(&self) -> &str {
        "md"
    }

    fn doc_style(&self) -> DocStyle {
        DocStyle::Tags
    }

    fn extract(&self, options: &Value, root: &Path) -> Result<Extracted, String> {
        let entries: Vec<extract::Entry> = match options.get("paths") {
            Some(paths) => serde_json::from_value(paths.clone()).map_err(|e| {
                format!("`paths` must be a list of paths or {{ path, module }} tables: {e}")
            })?,
            None => return Err("the `paths` option is missing".into()),
        };
        let args: Vec<String> = match options.get("args") {
            Some(args) => serde_json::from_value(args.clone())
                .map_err(|e| format!("`args` must be a list of strings: {e}"))?,
            None => Vec::new(),
        };
        let found = extract::extract(&entries, root, self.lang, &args)?;
        Ok(Extracted {
            items: found.items,
            files: found.files,
            diagnostics: found.diagnostics,
        })
    }

    fn objects(&self) -> Vec<(String, ItemKind)> {
        let common = [
            ("function", ItemKind::Function),
            ("struct", ItemKind::Struct),
            ("union", ItemKind::Union),
            ("enum", ItemKind::Enum),
            ("enumerator", ItemKind::Variant),
            ("member", ItemKind::Field),
            ("var", ItemKind::Variable),
            ("type", ItemKind::TypeAlias),
            ("macro", ItemKind::Macro),
        ];
        let cpp: &[(&str, ItemKind)] = match self.lang {
            Lang::C => &[],
            Lang::Cpp => &[
                ("class", ItemKind::Class),
                ("enum-class", ItemKind::Enum),
                ("enum-struct", ItemKind::Enum),
                ("concept", ItemKind::Other("concept".into())),
            ],
        };
        common
            .into_iter()
            .chain(cpp.iter().cloned())
            .map(|(n, k)| (n.to_owned(), k))
            .collect()
    }

    fn roles(&self) -> Vec<(String, Vec<ItemKind>)> {
        use ItemKind::*;
        let records = vec![Class, Struct, Union];
        let mut roles = vec![
            ("func", vec![Function, Method, Constructor]),
            ("member", vec![Field, Variable, Constant, Variant]),
            ("var", vec![Variable, Constant, Field]),
            ("data", vec![Variable, Constant]),
            ("macro", vec![Macro]),
            ("struct", records.clone()),
            ("union", vec![Union]),
            ("enum", vec![Enum]),
            ("enumerator", vec![Variant, Constant]),
            ("type", vec![TypeAlias, Class, Struct, Union, Enum]),
            ("obj", vec![]),
        ];
        if self.lang == Lang::Cpp {
            roles.extend([
                ("class", records),
                ("concept", vec![Other("concept".into())]),
                ("any", vec![]),
            ]);
        }
        roles.into_iter().map(|(n, k)| (n.to_owned(), k)).collect()
    }

    /// A hand-written declaration, as C writes it: `int add(int a, int
    /// b)`, `struct point`, `double x`, `MAX(a, b)`.
    fn describe(&self, kind: ItemKind, signature: &str, scope: &[String]) -> Option<Item> {
        let name = declared_name(signature)?;
        let mut path = scope.to_vec();
        path.extend(split_name(&name));
        let mut item = Item::new(kind, path);
        item.signature = Some(Signature {
            text: Some(signature.trim().to_owned()),
            ..Signature::default()
        });
        Some(item)
    }
}

/// The name a C or C++ declaration declares: the (qualified) name before
/// its parameters, or its last name before an initializer.
fn declared_name(signature: &str) -> Option<String> {
    let sig = signature.trim();
    let sig = sig.rsplit('\n').next().unwrap_or(sig);
    // An operator: its name runs to its parameters (`operator==`,
    // `operator()`), with what qualifies it.
    if let Some(at) = sig.find("operator")
        && !sig[at + 8..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    {
        let rest = &sig[at + 8..];
        let symbol = if rest.trim_start().starts_with("()") {
            "()"
        } else {
            rest.split('(').next().unwrap_or_default().trim()
        };
        let start = sig[..at]
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':' || c == '~'))
            .map_or(0, |i| i + 1);
        return Some(format!("{}operator{symbol}", &sig[start..at]));
    }
    // Before the parameters (outside template brackets), or before `=`,
    // `[` or a bit-field's `:`.
    let mut depth = 0i32;
    let mut end = sig.len();
    let bytes = sig.as_bytes();
    for (i, c) in sig.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            '(' if depth == 0 => {
                // `operator()`: the name includes its parentheses.
                if sig[..i].ends_with("operator") {
                    continue;
                }
                end = i;
                break;
            }
            '=' | '[' if depth == 0 => {
                end = i;
                break;
            }
            ':' if depth == 0
                && bytes.get(i + 1) != Some(&b':')
                && (i == 0 || bytes[i - 1] != b':') =>
            {
                end = i;
                break;
            }
            _ => {}
        }
    }
    let head = sig[..end].trim_end();
    // The last word, with its qualifiers (`ns::Point::norm`, `~Shape`,
    // `operator+`).
    let start = head
        .rfind(|c: char| c.is_whitespace() || c == '*' || c == '&')
        .map_or(0, |i| i + 1);
    let name = head[start..].trim_start_matches(['*', '&']);
    let name = if name.is_empty() {
        head.split_whitespace().last()?
    } else {
        name
    };
    (!name.is_empty()).then(|| name.to_owned())
}
