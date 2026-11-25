//! IntelliJ's Docker integration (its Docker plugin) for the Services tool window: the Docker
//! Engine API client ([`engine`]), the connections saved per IDE and the local daemon found as
//! IntelliJ finds it ([`connections`]), Settings › Build, Execution, Deployment › Docker
//! ([`settings_page`]) and Create Terminal's bridge to a shell in a container ([`exec`]). The
//! Services tool window's Docker node is `spork_run`'s.

// shortcut: Unix only (Unix sockets, termios); Windows needs Docker Desktop's named pipe
// (`npipe:////./pipe/docker_engine`) and a console bridge once Spork IDE builds there

pub mod connections;
pub mod engine;
pub mod exec;
pub mod settings_page;

pub use connections::{Connection, ConnectionStore, Kind};
pub use engine::Engine;
pub use exec::exec_if_asked;
pub use settings_page::{ConnectionForm, SettingsPage};
