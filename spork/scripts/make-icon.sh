#!/usr/bin/env bash
# Renders crates/spork/icon/spork.svg into crates/spork/icon/Spork.icns (macOS) and spork.png
# (512 px, Linux). Uses resvg, the SVG renderer GPUI itself uses; macOS's own one draws strokes
# wrong. Run it after changing the SVG; the outputs are committed.
set -euo pipefail
# the monorepo, whose workspace builds Spork
cd "$(dirname "$0")/../.."
tool=target/make-icon
mkdir -p "$tool/src"
cat > "$tool/Cargo.toml" <<'TOML'
[package]
name = "make-icon"
version = "0.0.0"
edition = "2024"
publish = false
[workspace]
[dependencies]
resvg = "0.45"
TOML
cat > "$tool/src/main.rs" <<'RS'
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let svg = std::fs::read(&args[1]).expect("read the svg");
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).expect("valid svg");
    for size in args[3..].iter().map(|s| s.parse::<u32>().expect("a size")) {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("size > 0");
        let scale = size as f32 / tree.size().width();
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        pixmap.save_png(format!("{}/{size}.png", args[2])).expect("write the png");
    }
}
RS
out=$(mktemp -d)
cargo run -q --release --manifest-path "$tool/Cargo.toml" -- crates/spork/icon/spork.svg "$out" 16 32 64 128 256 512 1024
set=$out/Spork.iconset
mkdir -p "$set"
for s in 16 32 128 256 512; do
  cp "$out/$s.png" "$set/icon_${s}x${s}.png"
  cp "$out/$((s * 2)).png" "$set/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set" -o crates/spork/icon/Spork.icns
cp "$out/512.png" crates/spork/icon/spork.png
rm -rf "$out"
echo "wrote crates/spork/icon/Spork.icns and spork.png"
