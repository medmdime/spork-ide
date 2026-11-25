//! SSH keys for connecting to git servers (docs/specs/M10 §4): list the public keys in
//! `~/.ssh`, generate an ed25519 key with `ssh-keygen`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct SshKey {
    /// The `.pub` file.
    pub(crate) path: PathBuf,
    /// Its one line, e.g. `ssh-ed25519 AAAA… you@example.com`.
    pub(crate) public: String,
}

/// `~/.ssh` (`%USERPROFILE%\.ssh` on Windows).
#[allow(dead_code)]
pub(crate) fn ssh_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".ssh"))
}

/// The public keys in `dir` (files ending in `.pub`), sorted by file name; unreadable files are skipped.
#[allow(dead_code)]
pub(crate) fn keys(dir: &Path) -> Vec<SshKey> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "pub") {
            continue;
        }
        // unreadable files are skipped
        if let Ok(content) = std::fs::read_to_string(&path) {
            out.push(SshKey {
                path,
                public: content.trim().to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    out
}

/// Generates `dir/<name>` (and `.pub`) as an ed25519 key with `ssh-keygen`; returns the public key.
/// An empty `passphrase` makes an unprotected key.
// shortcut: the passphrase is passed with -N, briefly visible to local `ps`; feed it through a pty when that matters.
#[allow(dead_code)]
#[allow(
    clippy::disallowed_methods,
    reason = "ssh-keygen, given its passphrase, returns at once with the key the dialog shows"
)]
pub(crate) fn generate(
    dir: &Path,
    name: &str,
    comment: &str,
    passphrase: &str,
) -> Result<SshKey, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("Use a plain file name, e.g. id_ed25519.".into());
    }
    let path = dir.join(name);
    if path.exists() {
        return Err(format!("{} already exists.", path.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let output = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-C", comment, "-N", passphrase, "-f"])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => return Err(format!("ssh-keygen isn't available: {e}")),
    };
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    // `with_extension` would replace an existing extension, so build the `.pub` path explicitly.
    let pub_path = PathBuf::from(format!("{}.pub", path.display()));
    let public = std::fs::read_to_string(&pub_path).map_err(|e| e.to_string())?;
    Ok(SshKey {
        path: pub_path,
        public: public.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{generate, keys};

    #[test]
    fn lists_public_keys() {
        let dir = std::env::temp_dir().join(format!("spork-ssh-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.pub"), "ssh-ed25519 BBB b\n").unwrap();
        std::fs::write(dir.join("a.pub"), "ssh-rsa AAA a\n").unwrap();
        std::fs::write(dir.join("a"), "PRIVATE").unwrap();
        let keys = keys(&dir);
        assert_eq!(keys.len(), 2);
        assert!(keys[0].path.ends_with("a.pub"));
        assert_eq!(keys[0].public, "ssh-rsa AAA a");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[allow(clippy::disallowed_methods, reason = "a test")]
    fn generates_ed25519() {
        let dir = std::env::temp_dir().join(format!("spork-ssh-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(generate(&dir, "../x", "c", "").is_err());
        if std::process::Command::new("ssh-keygen")
            .arg("-?")
            .output()
            .is_err()
        {
            eprintln!("ssh-keygen missing, skipping");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let k = generate(&dir, "id_test", "me@spork", "").unwrap();
        assert!(k.public.starts_with("ssh-ed25519 "));
        assert!(k.public.ends_with("me@spork"));
        assert!(generate(&dir, "id_test", "me@spork", "").is_err());
        assert_eq!(keys(&dir).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
