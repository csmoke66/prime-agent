//! `--mode daemon`: the supervisor process. The interactive client spawns this
//! mode (detached) when no daemon is listening.

use anyhow::Result;

use crate::config;

pub fn run_daemon_mode(daemon_socket: Option<&str>) -> Result<i32> {
    let socket_path = config::resolve_daemon_socket_path(daemon_socket);
    let agent_dir = config::get_agent_dir();
    let web_port = bh_core::settings::SettingsManager::create(&agent_dir, &agent_dir).get_web_server_port();
    let options = bh_daemon::supervisor::SupervisorOptions {
        socket_path: socket_path.clone(),
        agent_dir,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if let Some(port) = web_port {
        // The web UI is a daemon-wire client: it connects once the supervisor listens.
        runtime.spawn(crate::web_command::serve_for_daemon(socket_path, port));
    }
    runtime.block_on(bh_daemon::supervisor::run_supervisor(options))?;
    Ok(0)
}
