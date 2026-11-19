# lsdocs-apidoc-clang

Part of [lsdocs](https://github.com/lsdocs), a static-site generator whose
parsers, transformers and renderers are plugins.

`crates/apidoc-clang` (`lsdocs-apidoc-clang`, builtins `apidoc-c` and
`apidoc-cpp`): documents C and C++ headers with libclang, in the `c` and
`cpp` domains, Doxygen comments as Markdown. libclang (10 or later) is
loaded at run time; installing it on each platform is described in
`crates/apidoc-clang/README.md`. `examples/clang-sample` is the header
library its tests (and its API pages) document.

The plugin's settings and output are described in the lsdocs
documentation (https://lsdocs.github.io, "apidoc-c and apidoc-cpp").

## Known limitations

- C++20 concepts are not documented (the `clang` crate knows libclang
  10's cursor kinds; newer ones are `NotImplemented`).
- Overloads are one item: every signature, but the first documented
  overload's comment only.
- Template specializations are left out.
- Types in signatures are text, not links.

## TODO

- [ ] C++20 concepts, specializations, per-overload comments, linked
      types.
- [ ] Objective-C (and Objective-C++): libclang parses them too; an
      `apidoc-objc` with an `objc` domain (classes, protocols,
      categories, methods by selector) would fit the same crate.
