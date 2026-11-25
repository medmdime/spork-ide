//! The shelf: changes put aside as patches outside git, where the reference app keeps them (Settings ›
//! Version Control › Shelf): `<project>/.idea/shelf/<name>.xml` describes each shelved
//! changelist, `<name>/shelved.patch` holds its changes.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{GitError, Outcome, allow_conflicts, run, run_with_env, run_with_input};

/// One shelved changelist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelved {
    /// Its file name in the shelf folder, without `.xml`.
    pub name: String,
    pub description: String,
    /// When it was shelved, in milliseconds since 1970.
    pub date: i64,
    /// Unshelved with "Remove successfully applied files": kept as Already Unshelved.
    pub recycled: bool,
    /// Deleted: kept as Recently Deleted until it is restored.
    pub deleted: bool,
}

/// Where shelves go without a custom directory.
pub fn default_dir(root: &Path) -> PathBuf {
    root.join(".idea").join("shelf")
}

/// The project's shelf folder: the custom directory Change Shelves Location… set, or
/// `.idea/shelf`.
pub fn dir(root: &Path) -> PathBuf {
    custom_dir(root).unwrap_or_else(|| default_dir(root))
}

/// Where the reference app keeps the project's shelf location: its workspace file.
fn workspace_file(root: &Path) -> PathBuf {
    root.join(".idea").join("workspace.xml")
}

/// The workspace file's VcsManagerConfiguration component: where it starts, where its inner
/// part starts and ends (the same for an empty element) and where it ends.
fn vcs_component(xml: &str) -> Option<(usize, usize, usize, usize)> {
    let start = xml.find("<component name=\"VcsManagerConfiguration\"")?;
    let open_end = start + xml[start..].find('>')? + 1;
    if xml[..open_end].ends_with("/>") {
        return Some((start, open_end, open_end, open_end));
    }
    let close = open_end + xml[open_end..].find("</component>")?;
    Some((start, open_end, close, close + "</component>".len()))
}

/// The value of `<option name="{key}" value="…" />` in `xml`.
fn option(xml: &str, key: &str) -> Option<String> {
    let at = xml.find(&format!("<option name=\"{key}\""))?;
    attribute(&xml[at..at + xml[at..].find('>')?], "value")
}

/// The custom shelf directory, while it is the one used (the reference app's USE_CUSTOM_SHELF_PATH).
pub fn custom_dir(root: &Path) -> Option<PathBuf> {
    let xml = std::fs::read_to_string(workspace_file(root)).ok()?;
    let (_, inner, end, _) = vcs_component(&xml)?;
    let component = &xml[inner..end];
    if option(component, "USE_CUSTOM_SHELF_PATH")? != "true" {
        return None;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let path = option(component, "CUSTOM_SHELF_PATH")?
        .replace("$PROJECT_DIR$", &root.to_string_lossy())
        .replace("$USER_HOME$", &home);
    Some(PathBuf::from(path))
}

/// Change Shelves Location…: shelves are kept in `custom` from now on (`.idea/shelf` with
/// none), as the reference app's workspace file then says too; `move_shelves` takes those of the old
/// folder along.
pub fn set_dir(root: &Path, custom: Option<&Path>, move_shelves: bool) -> Result<(), GitError> {
    let old = dir(root);
    let file = workspace_file(root);
    let xml = match std::fs::read_to_string(&file) {
        Ok(xml) => xml,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n</project>\n"
                .to_owned()
        }
        Err(e) => return Err(io(e)),
    };
    let mut keys = vec!["USE_CUSTOM_SHELF_PATH", "MOVE_SHELVES"];
    let mut options = vec![
        format!(
            "    <option name=\"USE_CUSTOM_SHELF_PATH\" value=\"{}\" />",
            custom.is_some()
        ),
        format!("    <option name=\"MOVE_SHELVES\" value=\"{move_shelves}\" />"),
    ];
    if let Some(custom) = custom {
        keys.push("CUSTOM_SHELF_PATH");
        options.push(format!(
            "    <option name=\"CUSTOM_SHELF_PATH\" value=\"{}\" />",
            escape(&custom.to_string_lossy())
        ));
    }
    let (start, end, kept) = match vcs_component(&xml) {
        Some((start, inner, close, end)) => (start, end, &xml[inner..close]),
        None => {
            let at = xml.rfind("</project>").unwrap_or(xml.len());
            (at, at, "")
        }
    };
    let lines: Vec<String> = kept
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| {
            !keys
                .iter()
                .any(|key| line.contains(&format!("<option name=\"{key}\"")))
        })
        .map(str::to_owned)
        .chain(options)
        .collect();
    let component = format!(
        "<component name=\"VcsManagerConfiguration\">\n{}\n  </component>",
        lines.join("\n")
    );
    // a new component goes on a line of its own before </project>
    let (head, tail) = if end == start { ("  ", "\n") } else { ("", "") };
    let xml = format!("{}{head}{component}{tail}{}", &xml[..start], &xml[end..]);
    std::fs::create_dir_all(file.parent().unwrap_or(root)).map_err(io)?;
    std::fs::write(&file, xml).map_err(io)?;
    let new = dir(root);
    if !move_shelves || new == old || !old.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&new).map_err(io)?;
    for entry in std::fs::read_dir(&old).map_err(io)? {
        let entry = entry.map_err(io)?;
        std::fs::rename(entry.path(), new.join(entry.file_name())).map_err(io)?;
    }
    Ok(())
}

fn patch_file(root: &Path, name: &str) -> PathBuf {
    dir(root).join(name).join("shelved.patch")
}

fn io(e: std::io::Error) -> GitError {
    GitError::Other {
        code: None,
        stderr: e.to_string(),
    }
}

/// The shelved changelists, newest first; a folder that can't be read has none.
pub fn list(root: &Path) -> Vec<Shelved> {
    let Ok(entries) = std::fs::read_dir(dir(root)) else {
        return Vec::new();
    };
    let mut lists: Vec<Shelved> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?.to_owned();
            if path.extension()? != "xml" {
                return None;
            }
            parse(&name, &std::fs::read_to_string(&path).ok()?)
        })
        .collect();
    lists.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.name.cmp(&b.name)));
    lists
}

fn attribute(text: &str, key: &str) -> Option<String> {
    let start = text.find(&format!("{key}=\""))? + key.len() + 2;
    let len = text[start..].find('"')?;
    Some(unescape(&text[start..start + len]))
}

fn parse(name: &str, xml: &str) -> Option<Shelved> {
    let head = &xml[xml.find("<changelist")?..];
    let head = &head[..head.find('>')?];
    let description = xml
        .find("name=\"DESCRIPTION\"")
        .and_then(|at| attribute(&xml[at..], "value"))
        .unwrap_or_else(|| name.to_owned());
    Some(Shelved {
        name: name.to_owned(),
        description,
        date: attribute(head, "date")?.parse().ok()?,
        recycled: attribute(head, "recycled").as_deref() == Some("true"),
        deleted: attribute(head, "deleted").as_deref() == Some("true"),
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "&#10;")
}

fn unescape(text: &str) -> String {
    text.replace("&#10;", "\n")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// Writes `list`'s description file, as it is now.
pub fn update(root: &Path, list: &Shelved) -> Result<(), GitError> {
    let patch = patch_file(root, &list.name);
    let path = match patch.strip_prefix(root) {
        Ok(inside) => format!("$PROJECT_DIR$/{}", inside.display()),
        Err(_) => patch.display().to_string(),
    };
    let xml = format!(
        "<changelist name=\"{name}\" date=\"{date}\" recycled=\"{recycled}\" deleted=\"{deleted}\">\n  \
         <option name=\"PATH\" value=\"{path}\" />\n  \
         <option name=\"DESCRIPTION\" value=\"{description}\" />\n</changelist>\n",
        path = escape(&path),
        name = escape(&list.name),
        date = list.date,
        recycled = list.recycled,
        deleted = list.deleted,
        description = escape(&list.description),
    );
    std::fs::write(dir(root).join(format!("{}.xml", list.name)), xml).map_err(io)
}

/// The patch a shelved changelist holds.
pub fn patch(root: &Path, list: &Shelved) -> Result<String, GitError> {
    std::fs::read_to_string(patch_file(root, &list.name)).map_err(io)
}

/// The files a patch changes, each with its status letter (A, D, R or M).
pub fn files(patch: &str) -> Vec<(char, String)> {
    let mut files: Vec<(char, String)> = Vec::new();
    for line in patch.lines() {
        if let Some(paths) = line.strip_prefix("diff --git a/") {
            let path = paths.split_once(" b/").map_or(paths, |(_, b)| b);
            files.push(('M', path.to_owned()));
        } else if let Some(last) = files.last_mut() {
            if line.starts_with("new file mode") {
                last.0 = 'A';
            } else if line.starts_with("deleted file mode") {
                last.0 = 'D';
            } else if line.starts_with("rename from") {
                last.0 = 'R';
            }
        }
    }
    files
}

/// A new shelved changelist holding `patch`, named after `description` as the reference app names them.
pub fn save(root: &Path, description: &str, patch: &str) -> Result<Shelved, GitError> {
    let base: String = description
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(50)
        .collect();
    let base = if base.is_empty() {
        "shelved".into()
    } else {
        base
    };
    let name = (0..)
        .map(|n| match n {
            0 => base.clone(),
            n => format!("{base}_{n}"),
        })
        .find(|name| !dir(root).join(format!("{name}.xml")).exists())
        .unwrap_or(base);
    let date = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let file = patch_file(root, &name);
    std::fs::create_dir_all(file.parent().unwrap_or(&file)).map_err(io)?;
    // as the reference app sets a project up: the shelf stays out of the unversioned files
    let ignore = root.join(".idea").join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "# Default ignored files\n/shelf/\n").map_err(io)?;
    }
    std::fs::write(&file, patch).map_err(io)?;
    let list = Shelved {
        name,
        description: description.to_owned(),
        date,
        recycled: false,
        deleted: false,
    };
    update(root, &list)?;
    Ok(list)
}

/// Shelves the local changes of `paths` and reverts them, new files included.
pub fn shelve(root: &Path, description: &str, paths: &[&str]) -> Result<Shelved, GitError> {
    let mut add = vec!["add", "--intent-to-add", "--"];
    add.extend(paths);
    run(root, &add)?;
    let mut diff = vec!["diff", "HEAD", "--binary", "--"];
    diff.extend(paths);
    let patch = String::from_utf8_lossy(&run(root, &diff)?).into_owned();
    if patch.trim().is_empty() {
        return Err(GitError::Other {
            code: None,
            stderr: "No changes to shelve.".into(),
        });
    }
    let list = save(root, description, &patch)?;
    run_with_input(root, &["apply", "--reverse", "-"], Some(patch.as_bytes()))?;
    let mut reset = vec!["reset", "--quiet", "--"];
    reset.extend(paths);
    run(root, &reset)?;
    Ok(list)
}

/// A shelved changelist made from a patch file.
pub fn import(root: &Path, file: &Path) -> Result<Shelved, GitError> {
    let patch = std::fs::read_to_string(file).map_err(io)?;
    let description = file
        .file_stem()
        .map_or_else(|| "Imported".into(), |s| s.to_string_lossy().into_owned());
    save(root, &description, &patch)
}

/// Whether `text` reads as a patch (Apply Patch from Clipboard…): a unified diff's file header
/// and hunk, or git's file header.
pub fn is_patch(text: &str) -> bool {
    let has = |prefix: &str| text.lines().any(|line| line.starts_with(prefix));
    has("diff --git ") || (has("+++ ") && has("@@ "))
}

/// Applies `patch` (only its `paths` when there are some) to the working tree; a patch that no
/// longer applies cleanly is merged, leaving conflicts to resolve.
pub fn apply(root: &Path, patch: &str, paths: &[&str]) -> Result<Outcome, GitError> {
    let includes: Vec<String> = paths.iter().map(|p| format!("--include={p}")).collect();
    let apply = |three_way: bool| {
        let mut args = vec!["apply", "--whitespace=nowarn"];
        args.extend(includes.iter().map(String::as_str));
        if three_way {
            args.push("--3way");
        }
        args.push("-");
        run_with_input(root, &args, Some(patch.as_bytes()))
    };
    match apply(false) {
        Ok(_) => Ok(Outcome::Done),
        Err(_) => allow_conflicts(root, apply(true)),
    }
}

/// Applies a shelved changelist to the working tree; `remove` then keeps it as Already Unshelved.
pub fn unshelve(root: &Path, list: &Shelved, remove: bool) -> Result<Outcome, GitError> {
    let outcome = apply(root, &patch(root, list)?, &[])?;
    if remove && outcome == Outcome::Done {
        update(
            root,
            &Shelved {
                recycled: true,
                ..list.clone()
            },
        )?;
    }
    Ok(outcome)
}

/// `patch` without the files in `paths`.
fn without_files(patch: &str, paths: &[&str]) -> String {
    let mut kept = String::new();
    let mut keep = true;
    for line in patch.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix("diff --git a/") {
            let header = header.trim_end();
            let path = header.split_once(" b/").map_or(header, |(_, b)| b);
            keep = !paths.contains(&path);
        }
        if keep {
            kept.push_str(line);
        }
    }
    kept
}

/// Unshelve (and Unshelve and Remove) of some files of a shelved changelist: `remove` then
/// takes them out of it, and a changelist left with none is kept as Already Unshelved.
pub fn unshelve_files(
    root: &Path,
    list: &Shelved,
    paths: &[&str],
    remove: bool,
) -> Result<Outcome, GitError> {
    let patch = patch(root, list)?;
    let outcome = apply(root, &patch, paths)?;
    if remove && outcome == Outcome::Done {
        let rest = without_files(&patch, paths);
        match files(&rest).is_empty() {
            true => update(
                root,
                &Shelved {
                    recycled: true,
                    ..list.clone()
                },
            )?,
            false => std::fs::write(patch_file(root, &list.name), rest).map_err(io)?,
        }
    }
    Ok(outcome)
}

/// The diff from `paths` (all of them when none) as `list` shelved them to the files in the
/// working tree (Compare with Local): the shelved versions are built in an index of their own,
/// the repository's isn't touched.
pub fn compare_with_local(root: &Path, list: &Shelved, paths: &[&str]) -> Result<String, GitError> {
    let patch = patch(root, list)?;
    let shelved: Vec<String> = files(&patch).into_iter().map(|(_, path)| path).collect();
    let paths: Vec<&str> = match paths.is_empty() {
        true => shelved.iter().map(String::as_str).collect(),
        false => paths.to_vec(),
    };
    let index = std::env::temp_dir().join(format!("spork-shelf-{}.index", std::process::id()));
    let index_path = index.to_string_lossy().into_owned();
    let env = [("GIT_INDEX_FILE", index_path.as_str())];
    let includes: Vec<String> = paths.iter().map(|p| format!("--include={p}")).collect();
    let mut apply = vec!["apply", "--cached", "--whitespace=nowarn"];
    apply.extend(includes.iter().map(String::as_str));
    apply.push("-");
    let mut diff = vec!["diff", "--"];
    diff.extend(&paths);
    let diff = run_with_env(root, &["read-tree", "HEAD"], None, &env)
        .and_then(|_| run_with_env(root, &apply, Some(patch.as_bytes()), &env))
        .and_then(|_| run_with_env(root, &diff, None, &env));
    match std::fs::remove_file(&index) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
        _ => {}
    }
    Ok(String::from_utf8_lossy(&diff?).into_owned())
}

/// Deletes a shelved changelist for good, its patch with it.
pub fn remove(root: &Path, list: &Shelved) -> Result<(), GitError> {
    std::fs::remove_file(dir(root).join(format!("{}.xml", list.name))).map_err(io)?;
    match std::fs::remove_dir_all(dir(root).join(&list.name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::repo;

    #[test]
    fn shelve_list_unshelve_import() {
        let tmp = repo();
        let d = tmp.path();
        let write = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let read = |f: &str| std::fs::read_to_string(d.join(f)).unwrap();
        write("a.txt", "1\n");
        crate::stage(d, &["a.txt"]).unwrap();
        crate::commit(d, "base", false).unwrap();
        write("a.txt", "2\n");
        write("new.txt", "n\n");
        let shelved = shelve(d, "My \"work\" & more", &["a.txt", "new.txt"]).unwrap();
        assert_eq!(read("a.txt"), "1\n");
        assert!(!d.join("new.txt").exists());
        // the shelf itself isn't an unversioned file
        let status = crate::status(d).unwrap();
        assert!(
            status.iter().all(|e| !e.path.contains("shelf")),
            "{status:?}"
        );
        let lists = list(d);
        assert_eq!(lists, vec![shelved.clone()]);
        assert_eq!(lists[0].description, "My \"work\" & more");
        assert!(is_patch(&patch(d, &shelved).unwrap()));
        assert!(!is_patch("just some text\n+++ not a hunk"));
        let files = files(&patch(d, &shelved).unwrap());
        assert_eq!(
            files,
            vec![('M', "a.txt".to_owned()), ('A', "new.txt".to_owned())]
        );

        write("a.txt", "3\n");
        let compared = compare_with_local(d, &shelved, &["a.txt"]).unwrap();
        assert!(compared.contains("-2\n+3"), "{compared}");
        let whole = compare_with_local(d, &shelved, &[]).unwrap();
        assert!(
            whole.contains("-2\n+3") && whole.contains("new.txt"),
            "{whole}"
        );
        run(d, &["checkout", "--", "a.txt"]).unwrap();

        assert_eq!(unshelve(d, &shelved, true).unwrap(), Outcome::Done);
        assert_eq!(
            (read("a.txt").as_str(), read("new.txt").as_str()),
            ("2\n", "n\n")
        );
        assert!(list(d)[0].recycled);

        // a file at a time: Unshelve keeps it shelved, Unshelve and Remove takes it out
        run(d, &["checkout", "--", "a.txt"]).unwrap();
        std::fs::remove_file(d.join("new.txt")).unwrap();
        assert_eq!(
            unshelve_files(d, &shelved, &["a.txt"], false).unwrap(),
            Outcome::Done
        );
        assert_eq!(read("a.txt"), "2\n");
        assert!(!d.join("new.txt").exists());
        run(d, &["checkout", "--", "a.txt"]).unwrap();
        unshelve_files(d, &shelved, &["new.txt"], true).unwrap();
        assert_eq!(read("new.txt"), "n\n");
        assert_eq!(
            super::files(&patch(d, &shelved).unwrap()),
            vec![('M', "a.txt".to_owned())]
        );

        let file = d.join("fix.patch");
        std::fs::write(&file, patch(d, &shelved).unwrap()).unwrap();
        let imported = import(d, &file).unwrap();
        assert_eq!(imported.description, "fix");
        assert_eq!(list(d).len(), 2);
        remove(d, &imported).unwrap();
        assert_eq!(list(d).len(), 1);

        // Change Shelves Location…, its shelves moved; the reference app's workspace file keeps the rest
        let workspace = d.join(".idea").join("workspace.xml");
        std::fs::write(
            &workspace,
            "<?xml version=\"1.0\"?>\n<project version=\"4\">\n  <component name=\"VcsManagerConfiguration\">\n    <option name=\"LAST_COMMIT_MESSAGE\" value=\"x\" />\n  </component>\n</project>\n",
        )
        .unwrap();
        let custom = d.join("elsewhere");
        set_dir(d, Some(&custom), true).unwrap();
        assert_eq!(dir(d), custom);
        assert_eq!(list(d).len(), 1);
        assert!(
            !default_dir(d)
                .join(format!("{}.xml", shelved.name))
                .exists()
        );
        let xml = std::fs::read_to_string(&workspace).unwrap();
        assert!(
            xml.contains("LAST_COMMIT_MESSAGE") && xml.ends_with("</project>\n"),
            "{xml}"
        );
        set_dir(d, None, true).unwrap();
        assert_eq!((dir(d), list(d).len()), (default_dir(d), 1));
        let xml = std::fs::read_to_string(&workspace).unwrap();
        assert_eq!(xml.matches("<component").count(), 1, "{xml}");
    }
}
