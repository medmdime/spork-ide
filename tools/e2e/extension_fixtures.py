#!/usr/bin/env python3
"""Builds the end-to-end tests' fixture extensions (fixtures/extensions) as the extension registry
serves them: each one's folder as an archive holds it, its Rust part compiled to WebAssembly
(`extension.wasm`, against this repository's extension API, into the repository's target folder)
and its grammars compiled from C to WebAssembly (`grammars/<name>.wasm`). Nothing is downloaded:
the grammar is tree-sitter-json's parser (MIT) from the cargo registry, under the grammar's name,
compiled with the system's clang and linked with the Rust toolchain's wasm linker, where Zed's
builder downloads wasi-sdk.

As a module, `build()` builds them into target/e2e-extensions/<id> and returns those folders; as a
script it does the same and prints them.
"""

import glob
import os
import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SOURCES = HERE / "fixtures" / "extensions"
STAGED = ROOT / "target" / "e2e-extensions"
TARGET = "wasm32-wasip2"
# a fixture's files that aren't part of the extension
LEFT_OUT = {"Cargo.toml", "src"}
# each grammar: the C parser it is compiled from, a glob in the cargo registry
GRAMMARS = {"fixture": ("tree-sitter-json-*/src", "tree_sitter_json")}
CARGO_HOME = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
# rustup's cargo and rustc, as run.py finds them, wherever the harness is started from
ENV = dict(os.environ, PATH=f"{CARGO_HOME / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}",
           RUSTC_WRAPPER="")


def rust_toolchain_bin() -> pathlib.Path:
    """The folder of the Rust toolchain's own binaries, rust-lld among them."""
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], cwd=ROOT, env=ENV, check=True,
                             capture_output=True, text=True).stdout.strip()
    host = next(line.split()[1] for line in subprocess.run(
        ["rustc", "-vV"], cwd=ROOT, env=ENV, check=True, capture_output=True,
        text=True).stdout.splitlines() if line.startswith("host:"))
    return pathlib.Path(sysroot) / "lib" / "rustlib" / host / "bin"


def compile_grammar(name: str, out: pathlib.Path) -> None:
    pattern, symbol = GRAMMARS[name]
    found = sorted(glob.glob(str(CARGO_HOME / "registry" / "src" / "*" / pattern)))
    if not found:
        sys.exit(f"no {pattern} in the cargo registry: build the IDE first")
    src = pathlib.Path(found[-1])
    # wasi-sdk's libc isn't there: the parser only needs NULL from stdlib.h
    include = STAGED / ".include"
    include.mkdir(parents=True, exist_ok=True)
    (include / "stdlib.h").write_text("#include <stddef.h>\n")
    obj = STAGED / f".{name}.o"
    subprocess.run(["clang", "--target=wasm32-unknown-unknown", "-ffreestanding", "-fPIC", "-Os",
                    f"-D{symbol}=tree_sitter_{name}", "-I", str(include), "-I", str(src), "-c",
                    "-o", str(obj), str(src / "parser.c")], check=True)
    bin_dir = rust_toolchain_bin()
    subprocess.run([str(bin_dir / "rust-lld"), "-flavor", "wasm", "--experimental-pic", "-shared",
                    f"--export=tree_sitter_{name}", "-o", str(out), str(obj)], check=True,
                   env=dict(os.environ, DYLD_LIBRARY_PATH=str(bin_dir.parent.parent.parent)))


def build() -> dict[str, pathlib.Path]:
    target_dir = ROOT / "target"
    subprocess.run(["cargo", "build", "--quiet", "--release", "--target", TARGET,
                    "--manifest-path", str(SOURCES / "Cargo.toml"), "--target-dir", str(target_dir)],
                   check=True, cwd=ROOT, env=ENV)
    staged = {}
    for source in sorted(p for p in SOURCES.iterdir() if (p / "extension.toml").is_file()):
        out = STAGED / source.name
        shutil.rmtree(out, ignore_errors=True)
        shutil.copytree(source, out, ignore=lambda folder, names: [
            n for n in names if folder == str(source) and n in LEFT_OUT])
        if (source / "Cargo.toml").is_file():
            wasm = target_dir / TARGET / "release" / (source.name.replace("-", "_") + ".wasm")
            shutil.copy(wasm, out / "extension.wasm")
        manifest = (source / "extension.toml").read_text()
        for name in GRAMMARS:
            if f"[grammars.{name}]" in manifest:
                (out / "grammars").mkdir(exist_ok=True)
                compile_grammar(name, out / "grammars" / f"{name}.wasm")
        staged[source.name] = out
    return staged


if __name__ == "__main__":
    for id, folder in build().items():
        print(id, folder)
