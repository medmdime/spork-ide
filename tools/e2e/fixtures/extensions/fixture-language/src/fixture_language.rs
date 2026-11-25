//! The fixture language's server: checked for with `process:exec` (python3), downloaded with
//! `download_file` from the URL in its settings (`lsp.fixture-ls.settings.download_url`, the
//! test registry's), then run with python3.

use std::fs;

use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

const SERVER: &str = "lsp_fixture.py";

struct FixtureLanguage;

impl zed::Extension for FixtureLanguage {
    fn new() -> Self {
        FixtureLanguage
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let python = worktree
            .which("python3")
            .ok_or("python3 is not on the PATH")?;
        let version = zed::process::Command::new("python3")
            .arg("--version")
            .output()?;
        if version.status != Some(0) {
            return Err(format!(
                "python3 --version failed: {}",
                String::from_utf8_lossy(&version.stderr)
            ));
        }
        if !fs::metadata(SERVER).is_ok_and(|stat| stat.is_file()) {
            let url = LspSettings::for_worktree(language_server_id.as_ref(), worktree)?
                .settings
                .and_then(|settings| settings.get("download_url")?.as_str().map(str::to_owned))
                .ok_or("lsp.fixture-ls.settings.download_url is not set")?;
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&url, SERVER, zed::DownloadedFileType::Uncompressed)?;
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
        }
        let server = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(SERVER);
        Ok(zed::Command {
            command: python,
            args: vec![server.to_string_lossy().into_owned()],
            env: Vec::new(),
        })
    }
}

zed::register_extension!(FixtureLanguage);
