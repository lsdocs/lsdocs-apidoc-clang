# lsdocs-apidoc-clang

lsdocs plugins documenting C and C++ APIs from their headers, with
libclang: the `apidoc-c` and `apidoc-cpp` builtins. What they document,
their options and their domains are in the lsdocs documentation
(`docs/content/plugins/index.md`).

## libclang

The plugins load libclang (version 10 or later) when they run; it is not
linked. **Building** lsdocs needs no LLVM; **using** these plugins (and
running their tests) needs libclang installed.

### Windows

```powershell
winget install LLVM.LLVM
```

This installs `libclang.dll` in `C:\Program Files\LLVM\bin`, where it is
found. LLVM installed with Visual Studio ("C++ Clang tools for Windows")
or Scoop is found too.

### macOS

The Command Line Tools include libclang:

```sh
xcode-select --install
```

Or use Homebrew's LLVM, which is not linked where it is looked for, so
say where it is:

```sh
brew install llvm
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
```

### Linux

| Distribution | Command |
|---|---|
| Debian, Ubuntu | `sudo apt install libclang-dev` |
| Fedora, RHEL | `sudo dnf install clang-devel` |
| Arch | `sudo pacman -S clang` |
| openSUSE | `sudo zypper install clang-devel` |
| Alpine | `apk add clang-dev` |

### Elsewhere

libclang is looked for in `LIBCLANG_PATH` (a directory, or the file
itself), then through `llvm-config` on the `PATH`, then in the usual
install directories. With several versions installed, the newest is
taken; set `LIBCLANG_PATH` to choose.

## Standard headers

libclang finds its own builtin headers (`stddef.h`, `stdint.h`, ...)
beside itself. A C++ standard library (`<string>`, `<vector>`) comes
from the system: Visual Studio's on Windows, libstdc++ or libc++ on
Linux, the SDK on macOS. If a header's includes are not found, pass
what a compiler would need in the `args` option (`-I...`,
`-isysroot ...`, `--target=...`); unparsed declarations are reported as
warnings.

## Tests

The tests document the sample library in `examples/clang-sample` (which
the docs site documents too), so they need libclang as above.
