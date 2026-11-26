// The HTTP Request grammar's parser (tree-sitter-http 0.1.0's generated `parser.c`, MIT,
// `grammar/LICENSE`), compiled into the crate as the grammar crates compile theirs.
fn main() {
    cc::Build::new()
        .include("grammar")
        .file("grammar/parser.c")
        .warnings(false)
        .compile("tree-sitter-http");
    println!("cargo:rerun-if-changed=grammar/parser.c");
}
