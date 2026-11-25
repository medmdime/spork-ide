//! A context server (MCP) that the extension carries: it writes tools/e2e/mcp_fixture.py into
//! its work folder and runs it with macOS's python3. A context server's command is a path, a
//! relative one taken from the work folder: there is no worktree to look python3 up in.

use zed_extension_api::{self as zed, ContextServerId, Project, Result};

const SERVER: &str = include_str!("../../../../mcp_fixture.py");

struct FixtureMcp;

impl zed::Extension for FixtureMcp {
    fn new() -> Self {
        FixtureMcp
    }

    fn context_server_command(
        &mut self,
        _context_server_id: &ContextServerId,
        _project: &Project,
    ) -> Result<zed::Command> {
        std::fs::write("mcp_fixture.py", SERVER).map_err(|error| error.to_string())?;
        let server = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join("mcp_fixture.py");
        Ok(zed::Command {
            command: "/usr/bin/python3".into(),
            args: vec![server.to_string_lossy().into_owned()],
            env: Vec::new(),
        })
    }
}

zed::register_extension!(FixtureMcp);
