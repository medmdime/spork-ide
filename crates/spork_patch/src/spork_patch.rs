//! Partial patches for hunk / line staging (docs/specs/M07 §4).
//!
//! Parse one file's unified diff, pick change lines, and build a smaller patch that `git apply`
//! accepts. Forward patches (stage) keep unselected deletions as context and drop unselected
//! additions. Reverse patches (unstage, discard; applied with `--reverse`) do the mirror image.

/// Which side of the diff a line number refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Context,
    Removed,
    Added,
    /// `\ No newline at end of file`, belongs to the line before it.
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    /// The line as it appears in the patch, prefix and line ending included.
    pub raw: String,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

impl Line {
    pub fn is_change(&self) -> bool {
        matches!(self.kind, Kind::Removed | Kind::Added)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    old_start: usize,
    new_start: usize,
    /// Text after the closing `@@` (function context), line ending included.
    tail: String,
    lines: Vec<Line>,
}

/// One file's unified diff, as produced by `git diff` for a single path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    header: Vec<String>,
    hunks: Vec<Hunk>,
}

impl FilePatch {
    /// Parses the first file in `patch`. `None` for binary diffs or text without hunks.
    pub fn parse(patch: &str) -> Option<Self> {
        let mut header = Vec::new();
        let mut hunks: Vec<Hunk> = Vec::new();
        let (mut old, mut new) = (0, 0);
        for raw in patch.split_inclusive('\n') {
            if let Some(rest) = raw.strip_prefix("@@ -") {
                let (ranges, tail) = rest.split_once(" @@")?;
                let (old_range, new_range) = ranges.split_once(" +")?;
                let start = |r: &str| r.split(',').next()?.parse::<usize>().ok();
                old = start(old_range)?;
                new = start(new_range)?;
                hunks.push(Hunk {
                    old_start: old,
                    new_start: new,
                    tail: tail.to_owned(),
                    lines: Vec::new(),
                });
                continue;
            }
            let Some(hunk) = hunks.last_mut() else {
                if raw.starts_with("diff --git") && !header.is_empty() {
                    break; // only the first file
                }
                header.push(raw.to_owned());
                continue;
            };
            let line = match raw.as_bytes().first() {
                Some(b' ') => {
                    let l = Line {
                        kind: Kind::Context,
                        raw: raw.to_owned(),
                        old: Some(old),
                        new: Some(new),
                    };
                    (old, new) = (old + 1, new + 1);
                    l
                }
                Some(b'-') => {
                    let l = Line {
                        kind: Kind::Removed,
                        raw: raw.to_owned(),
                        old: Some(old),
                        new: None,
                    };
                    old += 1;
                    l
                }
                Some(b'+') => {
                    let l = Line {
                        kind: Kind::Added,
                        raw: raw.to_owned(),
                        old: None,
                        new: Some(new),
                    };
                    new += 1;
                    l
                }
                Some(b'\\') => Line {
                    kind: Kind::NoNewline,
                    raw: raw.to_owned(),
                    old: None,
                    new: None,
                },
                _ => break, // next file or trailing garbage
            };
            hunk.lines.push(line);
        }
        let binary = header
            .iter()
            .any(|h| h.starts_with("Binary files") || h.starts_with("GIT binary patch"));
        (!binary && !hunks.is_empty()).then_some(Self { header, hunks })
    }

    /// Every hunk line in display order (the index used by [`FilePatch::partial`]).
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.hunks.iter().flat_map(|h| &h.lines)
    }

    /// Partial patches can't express "delete part of a file"; those must be staged whole.
    pub fn supports_partial(&self) -> bool {
        !self
            .header
            .iter()
            .any(|h| h.starts_with("deleted file mode"))
    }

    /// Display-order indices between two line positions (inclusive, in either order), e.g. from a
    /// selection that runs from old line 12 to new line 15. `None` if a position isn't in the diff.
    pub fn range(
        &self,
        a: (Side, usize),
        b: (Side, usize),
    ) -> Option<std::ops::RangeInclusive<usize>> {
        let find = |(side, n): (Side, usize)| {
            self.lines().position(|l| match side {
                Side::Old => l.old == Some(n),
                Side::New => l.new == Some(n),
            })
        };
        let (a, b) = (find(a)?, find(b)?);
        Some(a.min(b)..=a.max(b))
    }

    /// Builds a patch containing only the change lines for which `selected(index, line)` is true.
    /// `reverse` builds it for `git apply --reverse` (unstage, discard). `None` if nothing is selected.
    pub fn partial(
        &self,
        selected: impl Fn(usize, &Line) -> bool,
        reverse: bool,
    ) -> Option<String> {
        let mut out: String = self.header.concat();
        let mut shift: isize = 0; // new-side offset caused by earlier emitted hunks
        let mut index = 0;
        let mut any = false;
        for hunk in &self.hunks {
            let (mut body, mut old_n, mut new_n, mut changed) =
                (String::new(), 0usize, 0usize, false);
            let mut kept_previous = false;
            for line in &hunk.lines {
                let pick = line.is_change() && selected(index, line);
                index += 1;
                let as_context = |l: &Line| format!(" {}", &l.raw[1..]);
                let emitted = match (line.kind, pick, reverse) {
                    (Kind::Context, ..) => Some(line.raw.clone()),
                    (Kind::Removed | Kind::Added, true, _) => Some(line.raw.clone()),
                    // unselected change that already exists on the side being patched stays as context
                    (Kind::Removed, false, false) | (Kind::Added, false, true) => {
                        Some(as_context(line))
                    }
                    (Kind::Removed, false, true) | (Kind::Added, false, false) => None,
                    (Kind::NoNewline, ..) => kept_previous.then(|| line.raw.clone()),
                };
                if line.kind != Kind::NoNewline {
                    kept_previous = emitted.is_some();
                }
                let Some(text) = emitted else { continue };
                match text.as_bytes()[0] {
                    b' ' => (old_n, new_n) = (old_n + 1, new_n + 1),
                    b'-' => old_n += 1,
                    b'+' => new_n += 1,
                    _ => {}
                }
                changed |= pick;
                body.push_str(&text);
            }
            if !changed {
                continue;
            }
            any = true;
            // The side git matches against the current content keeps its original start (old for
            // forward, new for --reverse); the other side shifts by what earlier hunks changed.
            // Zero-length ranges point at the line *before* the change, per the unified format.
            let edge = |len: usize, other: usize| {
                isize::from(len > 0 && other == 0) - isize::from(len == 0 && other > 0)
            };
            let (old_start, new_start) = if reverse {
                let new = hunk.new_start as isize;
                (new + shift + edge(old_n, new_n), new)
            } else {
                let old = hunk.old_start as isize;
                (old, old + shift + edge(new_n, old_n))
            };
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@{}",
                old_start.max(0),
                old_n,
                new_start.max(0),
                new_n,
                hunk.tail
            ));
            out.push_str(&body);
            shift += if reverse {
                old_n as isize - new_n as isize
            } else {
                new_n as isize - old_n as isize
            };
        }
        any.then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/f.txt b/f.txt
index 1111111..2222222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,4 +1,4 @@ fn top()
 a
-b
+B
 c
 d
@@ -10,3 +10,4 @@
 j
+k2
 k
 l
";

    #[test]
    fn parses_lines_with_numbers() {
        let p = FilePatch::parse(PATCH).unwrap();
        let kinds: Vec<_> = p.lines().map(|l| l.kind).collect();
        assert_eq!(kinds.len(), 9);
        let b = p.lines().find(|l| l.raw == "-b\n").unwrap();
        assert_eq!((b.old, b.new), (Some(2), None));
        let k2 = p.lines().find(|l| l.raw == "+k2\n").unwrap();
        assert_eq!((k2.old, k2.new), (None, Some(11)));
        assert!(p.supports_partial());
    }

    #[test]
    fn stage_one_hunk_only() {
        let p = FilePatch::parse(PATCH).unwrap();
        let out = p.partial(|_, l| l.raw == "+k2\n", false).unwrap();
        assert!(
            out.ends_with("@@ -10,3 +10,4 @@\n j\n+k2\n k\n l\n"),
            "{out}"
        );
        assert!(!out.contains("-b"));
    }

    #[test]
    fn stage_only_the_addition_keeps_the_deletion_as_context() {
        let p = FilePatch::parse(PATCH).unwrap();
        let out = p.partial(|_, l| l.raw == "+B\n", false).unwrap();
        // "-b" unselected → context; result keeps b and adds B
        assert!(
            out.contains("@@ -1,4 +1,5 @@ fn top()\n a\n b\n+B\n c\n d\n"),
            "{out}"
        );
    }

    #[test]
    fn second_hunk_start_shifts_by_earlier_hunks() {
        let p = FilePatch::parse(PATCH).unwrap();
        // stage only "-b": first hunk removes a line, so the second hunk moves up by one
        let out = p
            .partial(|_, l| l.kind == Kind::Removed || l.raw == "+k2\n", false)
            .unwrap();
        assert!(out.contains("@@ -1,4 +1,3 @@"), "{out}");
        assert!(out.contains("@@ -10,3 +9,4 @@"), "{out}");
    }

    #[test]
    fn reverse_keeps_unselected_additions_as_context() {
        let p = FilePatch::parse(PATCH).unwrap();
        let out = p.partial(|_, l| l.raw == "-b\n", true).unwrap();
        assert!(
            out.contains("@@ -1,5 +1,4 @@ fn top()\n a\n-b\n B\n c\n d\n"),
            "{out}"
        );
    }

    #[test]
    fn nothing_selected_is_none() {
        let p = FilePatch::parse(PATCH).unwrap();
        assert_eq!(p.partial(|_, _| false, false), None);
    }

    #[test]
    fn range_maps_positions_to_display_indices() {
        let p = FilePatch::parse(PATCH).unwrap();
        assert_eq!(p.range((Side::Old, 2), (Side::New, 2)), Some(1..=2));
        assert_eq!(p.range((Side::New, 11), (Side::Old, 2)), Some(1..=6));
        assert_eq!(p.range((Side::Old, 99), (Side::New, 1)), None);
    }

    #[test]
    fn no_newline_marker_follows_its_line() {
        let patch = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n";
        let p = FilePatch::parse(patch).unwrap();
        // stage only the deletion: "+c" dropped, so its marker goes too; "-b" keeps its marker
        let out = p.partial(|_, l| l.kind == Kind::Removed, false).unwrap();
        assert!(
            out.ends_with("@@ -1,2 +1,1 @@\n a\n-b\n\\ No newline at end of file\n"),
            "{out}"
        );
    }

    #[test]
    fn deleted_files_and_binaries_are_whole_file_only() {
        let deleted = "diff --git a/f b/f\ndeleted file mode 100644\n--- a/f\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
        assert!(!FilePatch::parse(deleted).unwrap().supports_partial());
        let binary = "diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n";
        assert_eq!(FilePatch::parse(binary), None);
    }

    #[test]
    fn keeps_crlf_bytes() {
        let patch = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n";
        let out = FilePatch::parse(patch)
            .unwrap()
            .partial(|_, l| l.kind == Kind::Added, false)
            .unwrap();
        assert!(out.contains(" a\r\n b\r\n+c\r\n"), "{out:?}");
    }
}
