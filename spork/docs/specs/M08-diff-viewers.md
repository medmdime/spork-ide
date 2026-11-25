# M08 — Diff, file viewers, blame, file history

Fork surfaces: FileViewHeader, DiffPopUpWindow, BinaryContentView, BinaryDiffView, OnionSkinImageDiffView,
SwipeImageDiffView, SubmoduleDiffView, BlameController, FileHistoryController. Phase: P1 (text), P4 (image/binary/submodule).
Components: `Diff`, `Editor` (read-only, gutter, folding, Tree-sitter), `Image`, `Slider`, `Toggle`, `Toolbar`, `VirtualList`.

## 1. Text diff
Source: `git diff --no-color --no-ext-diff -z --find-renames -U<ctx> [-w] <a> <b> -- <path>` (exact git semantics: textconv, LFS, attributes).
Parsed into `spork_patch::FileDiff{hunks[], binary, mode change, rename, similarity}`.
Word-level highlight: `imara-diff` on paired `-`/`+` line blocks (only when block ≤ 200 lines).
Syntax highlight: Tree-sitter by extension/shebang via gpui-kit `Editor` (pref to disable; auto-off for files > 1 MB).

## 2. Header toolbar (each toggle persisted per user, not per file)
| Toggle | Effect |
|---|---|
| Fewer / More context lines | `-U` 0..∞ in steps (default 3) |
| Next / Previous change | jump between hunks (`]` / `[` keys); across files at ends |
| Ignore whitespace | `-w` |
| Show entire file | `-U<large>` → full file with changes marked |
| Show invisible characters | render tabs/spaces/EOL (CR shown as `␍`) |
| Side-by-side | 2 synced `Editor` panes vs unified |
| Word wrap | soft wrap |
| Highlight pixels (images) | red overlay of differing pixels |
Large diffs (> 20k lines or > 5 MB): show summary + "Load diff" button.

## 3. Binary & images
Non-image binary: sizes old/new, "Save as…" (`git cat-file blob <oid>` → file), "Open with default app".
Images (png, jpg, gif, webp, bmp, ico, svg rendered, psd thumbnail via OS if possible): modes **Side-by-side** (with dimensions/sizes), **Swipe** (draggable divider), **Onion skin** (`Slider` opacity), **Highlight pixels**.
LFS pointer files: fetch real content with `git lfs smudge` (if object present) or show "LFS object not downloaded · Fetch".

## 4. Submodule diff
"Submodule `<path>` changed `<old7>..<new7>`" + list of commits in range (`git -C <sub> log --oneline old..new`), buttons **Update submodule** (`git submodule update --init -- <path>`), **Open submodule** (new tab).

## 5. Blame
`git blame --porcelain -M -C [-w] <rev> -- <path>` streamed. Gutter columns: SHA (coloured by age), author, date, summary; consecutive lines from the same commit grouped. Click → select commit in M05; "Blame previous revision" on a line → re-run at `<c>^` with the line followed. Search in file.

## 6. File history
`git log --follow --format=<row fmt> -z -- <path>` (+ `--name-status` to show renames). List left, diff right (that commit's change to the file, following renames). Actions: open file at revision, save as, checkout this version (`git restore --source=<c> -- <path>`, confirm), blame at revision.

## 7. External tools
"Open in external diff tool" → `git difftool -y [--tool=<t>] <a> <b> -- <path>` or custom tool command with `$LOCAL $REMOTE` (M11 §5 editor).

## Edge cases
Mixed line endings, files without trailing newline, non-UTF-8 encodings (detect via `encoding_rs`/`chardetng`; respect `working-tree-encoding`), very long lines (truncate render at 10k chars), symlinks (show target change), mode changes only, rename with 100% similarity (no hunks), deleted file → show old content in red.

## Tests
Parser snapshot suite from real `git diff` output for every edge case above; image diff fixture pairs; blame of a file with rename history.

## Open questions
1. Which image formats Fork diffs visually. 2. Default context-line steps. 3. Whether "Show entire file" uses `-U` or a separate full-file render.
