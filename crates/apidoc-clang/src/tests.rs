#![allow(clippy::unwrap_used, clippy::panic)]

//! These load libclang (10 or later), which must be installed.

use std::path::PathBuf;

use lsdocs_apidoc::{Item, ItemKind, Visibility};
use lsdocs_apidoc_plugin::{Extracted, Language};
use lsdocs_plugin_sdk::doctree::{Node, plain_text};
use lsdocs_plugin_sdk::protocol::{InitializeParams, SiteInfo};
use lsdocs_plugin_sdk::testing::{context_with_parsers, run_transform_in};
use serde_json::{Value, json};

use super::{CFamily, Lang, declared_name, plugin_cpp};

/// The sample library the docs site documents.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/clang-sample")
}

const C: CFamily = CFamily { lang: Lang::C };
const CPP: CFamily = CFamily { lang: Lang::Cpp };

fn extract(language: CFamily, options: Value) -> Extracted {
    language.extract(&options, &root()).unwrap()
}

fn find<'a>(language: CFamily, items: &'a [Item], path: &str) -> &'a Item {
    items
        .iter()
        .flat_map(Item::walk)
        .find(|i| i.qualified(language.separator()) == path)
        .unwrap_or_else(|| panic!("no {path}"))
}

fn sig(language: CFamily, items: &[Item], path: &str) -> String {
    language.signature(find(language, items, path))
}

#[test]
fn extracts_c() {
    let found = extract(C, json!({"paths": ["include/geo/vec.h"]}));
    assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);
    assert_eq!(found.files, ["include/geo/vec.h"]);
    let items = &found.items;
    let module = &items[0];
    assert_eq!(module.qualified("."), "vec");
    assert_eq!(C.signature(module), "#include \"vec.h\"");
    assert_eq!(
        module.docs.as_deref(),
        Some("Vectors, in C, for the documentation site and apidoc-clang's tests.")
    );
    let names: Vec<&str> = module.members.iter().map(Item::name).collect();
    // No include guard; the anonymous enum's enumerators are constants.
    assert_eq!(
        names,
        [
            "VEC_MAX",
            "vec_t",
            "vec_pool",
            "vec_order",
            "VEC_SORTED",
            "VEC_UNIQUE",
            "vec_new",
            "vec_free",
            "vec_sort",
            "vec_visit",
            "vec_count"
        ]
    );
    // A typedef of an anonymous struct is the struct.
    let vec = find(C, items, "vec.vec_t");
    assert_eq!(vec.kind, ItemKind::Struct);
    assert_eq!(C.signature(vec), "typedef struct { ... } vec_t");
    let fields: Vec<String> = vec.members.iter().map(|m| C.signature(m)).collect();
    assert_eq!(fields, ["double *data", "size_t len"]);
    assert_eq!(
        find(C, items, "vec.vec_t.data").docs.as_deref(),
        Some("The numbers.")
    );
    // An opaque type is its typedef.
    assert_eq!(find(C, items, "vec.vec_pool").kind, ItemKind::TypeAlias);
    assert_eq!(
        sig(C, items, "vec.vec_pool"),
        "typedef struct vec_pool vec_pool"
    );
    assert_eq!(sig(C, items, "vec.VEC_MAX"), "#define VEC_MAX 1024");
    assert_eq!(
        find(C, items, "vec.VEC_MAX").docs.as_deref(),
        Some("The largest length.")
    );
    assert_eq!(
        find(C, items, "vec.vec_order.VEC_DESC").value.as_deref(),
        Some("1")
    );
    assert_eq!(find(C, items, "vec.VEC_UNIQUE").kind, ItemKind::Constant);
    assert_eq!(sig(C, items, "vec.vec_new"), "vec_t *vec_new(size_t len)");
    // Doxygen's markup, as Markdown; its sections stay for the parser.
    assert_eq!(
        find(C, items, "vec.vec_new").docs.as_deref(),
        Some(
            "Makes a vector.\n\n\\param len How many numbers; at most {c:obj}`VEC_MAX`.\n\\return A vector of `len` zeros, or `NULL` when out of memory.\n\\sa vec_free"
        )
    );
    assert_eq!(
        sig(C, items, "vec.vec_visit"),
        "typedef int (*vec_visit)(double value, void *data)"
    );
    assert_eq!(sig(C, items, "vec.vec_count"), "extern size_t vec_count");
    assert_eq!(find(C, items, "vec.vec_count").kind, ItemKind::Variable);
}

#[test]
fn extracts_cpp() {
    let found = extract(
        CPP,
        json!({"paths": [{"path": "include/geo", "module": "geo"}]}),
    );
    assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);
    // A directory: every header, named by their paths in it.
    let modules: Vec<String> = found.items.iter().map(|i| i.qualified("::")).collect();
    assert_eq!(modules, ["geo/shapes", "geo/vec"]);
    assert_eq!(found.files, ["include/geo/shapes.hpp", "include/geo/vec.h"]);
    let items = &found.items;
    let module = &items[0];
    assert_eq!(CPP.signature(module), "#include \"shapes.hpp\"");
    let ns = "geo/shapes::geo";
    let at = |name: &str| format!("{ns}::{name}");
    assert_eq!(find(CPP, items, ns).kind, ItemKind::Namespace);
    assert_eq!(
        sig(CPP, items, "geo/shapes::GEO_PAINT"),
        "#define GEO_PAINT(shape, color) ((shape).paint(color))"
    );
    assert_eq!(
        sig(CPP, items, &at("Color")),
        "enum class Color : unsigned char"
    );
    let colors: Vec<(String, Option<String>)> = find(CPP, items, &at("Color"))
        .members
        .iter()
        .map(|m| (CPP.signature(m), m.docs.clone()))
        .collect();
    assert_eq!(
        colors,
        [
            ("Red = 1".to_owned(), Some("Warm.".to_owned())),
            ("Green".to_owned(), Some("Calm.".to_owned())),
            ("Blue = 7".to_owned(), Some("Cool.".to_owned())),
        ]
    );
    assert_eq!(sig(CPP, items, &at("Point::x")), "double x = 0");
    assert_eq!(
        sig(CPP, items, &at("Point::norm")),
        "double norm() const noexcept"
    );
    // Classes: bases, access, overloads as one item, constructors.
    let circle = find(CPP, items, &at("Circle"));
    assert_eq!(CPP.signature(circle), "class Circle final : public Shape");
    assert_eq!(circle.bases, ["Shape"]);
    let shape: Vec<(String, ItemKind, Visibility)> = find(CPP, items, &at("Shape"))
        .members
        .iter()
        .map(|m| (CPP.signature(m), m.kind.clone(), m.visibility))
        .collect();
    assert_eq!(
        shape,
        [
            (
                "explicit Shape(Point center)\nShape(const Shape &other) = default".to_owned(),
                ItemKind::Constructor,
                Visibility::Public
            ),
            (
                "virtual ~Shape()".to_owned(),
                ItemKind::Method,
                Visibility::Public
            ),
            (
                "virtual double area() const = 0".to_owned(),
                ItemKind::Method,
                Visibility::Public
            ),
            (
                "void paint(Color color, int times = 1)\nvoid paint(const char *name)".to_owned(),
                ItemKind::Method,
                Visibility::Public
            ),
            (
                "static unsigned long count".to_owned(),
                ItemKind::Field,
                Visibility::Public
            ),
            (
                "Point center_".to_owned(),
                ItemKind::Field,
                Visibility::Protected
            ),
            (
                "int coats_ = 0".to_owned(),
                ItemKind::Field,
                Visibility::Private
            ),
        ]
    );
    // An override without a comment has the overridden one's.
    assert_eq!(
        find(CPP, items, &at("Circle::area")).docs,
        find(CPP, items, &at("Shape::area")).docs
    );
    // Templates: the head on its own line.
    assert_eq!(
        sig(CPP, items, &at("largest")),
        "template <typename It>\nIt largest(It first, It last)"
    );
    assert_eq!(
        sig(CPP, items, &at("List")),
        "template <typename T>\nstruct List"
    );
    assert_eq!(
        sig(CPP, items, &at("Registry")),
        "using Registry = List<Shape *>"
    );
    assert_eq!(
        find(CPP, items, &at("Sides::value")).kind,
        ItemKind::Constant
    );
    assert_eq!(
        sig(CPP, items, &at("pi")),
        "constexpr double pi = 3.14159265358979"
    );
    assert_eq!(
        find(CPP, items, &at("detail")).visibility,
        Visibility::Private
    );
}

#[test]
fn reports_problems() {
    let dir = std::env::temp_dir().join(format!("lsdocs-clang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bad.h"),
        "#include \"missing.h\"\n/** Doc. */\nint f(;\n",
    )
    .unwrap();
    let found = C
        .extract(&json!({"paths": ["bad.h", "none.h"]}), &dir)
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let messages: Vec<&str> = found
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages[0].starts_with("none.h: no such file"),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.starts_with("bad.h:1: ") && m.contains("missing.h")),
        "{messages:?}"
    );
    assert!(C.extract(&json!({}), &dir).is_err());
}

#[test]
fn describes_declarations() {
    for (sig, name) in [
        ("int add(int a, int b)", "add"),
        ("const char *name(void)", "name"),
        ("struct point", "point"),
        ("double x = 0", "x"),
        ("unsigned flags : 3", "flags"),
        ("int values[4]", "values"),
        ("MAX(a, b)", "MAX"),
        (
            "template <typename T>\nT geo::Shape::largest(T a, T b) const",
            "geo::Shape::largest",
        ),
        ("bool operator==(const Point &other) const", "operator=="),
        ("void operator()(int x)", "operator()"),
        ("virtual ~Shape()", "~Shape"),
        ("std::vector<int> &values()", "values"),
    ] {
        assert_eq!(declared_name(sig).as_deref(), Some(name), "{sig}");
    }
    let item = CPP
        .describe(ItemKind::Function, "int add(int a)", &["m".into()])
        .unwrap();
    assert_eq!(item.qualified("::"), "m::add");
    assert_eq!(CPP.signature(&item), "int add(int a)");
}

#[tokio::test]
async fn documents_headers_with_references() {
    let page = "```{cpp:currentmodule} shapes\n```\n\nSee {cpp:class}`geo::Circle`, {cpp:func}`~geo::Shape::paint` and {cpp:enumerator}`Color::Blue`.\n\n```{cpp:auto} geo::Point\n```\n";
    let plugin = plugin_cpp();
    let init = InitializeParams {
        site: SiteInfo {
            root: Some(root().to_string_lossy().into_owned()),
            ..SiteInfo::default()
        },
        options: json!({"paths": ["include/geo/shapes.hpp"]}),
        ..InitializeParams::default()
    };
    let declared = plugin.capabilities_for(&init).syntax;
    let (guide, diagnostics) =
        lsdocs_parser_markdown::parse_with_syntax("guide.md", page, &declared);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ctx = context_with_parsers(
        &plugin,
        &init,
        vec![
            lsdocs_parser_markdown::plugin(),
            lsdocs_parser_rst::plugin(),
        ],
    );
    let out = run_transform_in(&plugin, &ctx, vec![guide]).await.unwrap();
    for (doc, diagnostics) in &out {
        assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", doc.source);
    }
    let sources: Vec<&str> = out.iter().map(|(d, _)| d.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "guide.md",
            "api/cpp/index.md",
            "api/cpp/shapes.md",
            // `detail` is private: no page.
            "api/cpp/shapes.geo.md"
        ]
    );
    let links = |nodes: &[Node]| -> Vec<String> {
        nodes
            .iter()
            .flat_map(Node::descendants)
            .filter_map(|n| match n {
                Node::Link(l) => Some(format!("{} -> {}", plain_text(&l.children), l.url)),
                _ => None,
            })
            .collect()
    };
    let (guide, _) = &out[0];
    assert_eq!(
        links(&guide.children),
        [
            "geo::Circle -> api/cpp/shapes.geo.md#cpp.shapes.geo.Circle",
            "paint() -> api/cpp/shapes.geo.md#cpp.shapes.geo.Shape.paint",
            "Color::Blue -> api/cpp/shapes.geo.md#cpp.shapes.geo.Color.Blue"
        ]
    );
    assert!(guide.descendants().any(
        |n| matches!(n, Node::CodeBlock(c) if c.value == "struct Point" && c.language.as_deref() == Some("cpp"))
    ));
    let (geo, _) = &out[3];
    let all = links(&geo.children);
    assert!(
        all.contains(&"Circle -> #cpp.shapes.geo.Circle".to_owned()),
        "{all:?}"
    );
    let text = plain_text(&geo.children);
    for expected in [
        "Parameters",
        "How many coats.",
        "Returns",
        "The area, in square units.",
        "Raises",
        "invalid_argument",
        "Type parameters",
        "An iterator over shape pointers.",
    ] {
        assert!(text.contains(expected), "no {expected:?} in {text}");
    }
    assert!(
        geo.descendants()
            .any(|n| matches!(n, Node::Admonition(a) if a.kind == "deprecated"))
    );
}
