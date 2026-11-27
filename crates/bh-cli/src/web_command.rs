//! `beehive web [--host H] [--port N] [--open]`: serves the browser UI next to the TUI. It ensures the
//! daemon is running, binds the loopback listener, prints the token URL, and runs `bh-web` until
//! interrupted.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};

const DEFAULT_PORT: u16 = 7878;

#[derive(Debug, PartialEq, Eq)]
struct WebArgs {
    host: IpAddr,
    port: Option<u16>,
    open: bool,
    daemon_socket: Option<String>,
    cwd: Option<String>,
}

fn parse_web_args(args: &[String]) -> Result<WebArgs> {
    let mut parsed = WebArgs {
        host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: None,
        open: false,
        daemon_socket: None,
        cwd: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| anyhow!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--open" => parsed.open = true,
            "--port" => {
                parsed.port = Some(
                    value("--port")?
                        .parse()
                        .context("--port expects a number")?,
                );
            }
            "--host" => {
                parsed.host = value("--host")?
                    .parse()
                    .context("--host expects an IP address")?
            }
            "--daemon-socket" => parsed.daemon_socket = Some(value("--daemon-socket")?),
            "--cwd" => parsed.cwd = Some(value("--cwd")?),
            other => bail!("Unknown web option: {other}"),
        }
    }
    Ok(parsed)
}

/// Host header values this listener answers to: its own address, plus the loopback names when it is
/// loopback (anything else is a DNS-rebinding attempt).
fn allowed_hosts(addr: SocketAddr) -> Vec<String> {
    let port = addr.port();
    let mut hosts = vec![SocketAddr::new(addr.ip(), port).to_string()];
    if addr.ip().is_loopback() {
        hosts.extend([
            format!("localhost:{port}"),
            format!("127.0.0.1:{port}"),
            format!("[::1]:{port}"),
        ]);
    }
    hosts
}

/// The browser login token, kept in `<agent-dir>/web-token` (mode 0600) so the URL stays the same
/// across daemon restarts and the desktop app can read it.
pub(crate) fn web_token(agent_dir: &std::path::Path) -> Result<String> {
    let path = agent_dir.join("web-token");
    if let Ok(token) = std::fs::read_to_string(&path) {
        let token = token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    std::fs::create_dir_all(agent_dir)?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, &token)?;
    std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::rename(&temp, &path)?;
    Ok(token)
}

async fn serve_on(
    listener: tokio::net::TcpListener,
    socket_path: PathBuf,
    cwd: PathBuf,
    token: String,
) -> Result<()> {
    let addr = listener.local_addr()?;
    let agent_dir = crate::config::get_agent_dir();
    let host = crate::web_host::CliWebHost::new(cwd.clone(), agent_dir.clone());
    bh_web::serve(
        listener,
        bh_web::WebOptions {
            socket_path,
            agent_dir,
            cwd,
            token,
            allowed_hosts: allowed_hosts(addr),
        },
        host,
    )
    .await
}

/// The daemon's web UI: serves on loopback `port` for as long as the daemon runs. A busy port is
/// reported and skipped; the daemon never fails because of the web UI.
pub(crate) async fn serve_for_daemon(socket_path: PathBuf, port: u16) {
    let agent_dir = crate::config::get_agent_dir();
    let served = async {
        let token = web_token(&agent_dir)?;
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .with_context(|| format!("port {port} is in use"))?;
        let home = dirs_home();
        serve_on(listener, socket_path, home, token).await
    };
    if let Err(error) = served.await {
        eprintln!("bh-cli: web UI not served: {error:#}");
    }
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

async fn run(args: WebArgs) -> Result<()> {
    let cwd = match &args.cwd {
        Some(cwd) => PathBuf::from(cwd),
        None => std::env::current_dir()?,
    };
    let socket_path = crate::config::resolve_daemon_socket_path(args.daemon_socket.as_deref());
    crate::interactive_mode::ensure_daemon_running(&socket_path, &cwd).await?;
    let token = web_token(&crate::config::get_agent_dir())?;
    let port = args.port.unwrap_or(DEFAULT_PORT);
    let listener = match tokio::net::TcpListener::bind((args.host, port)).await {
        Ok(listener) => listener,
        // The daemon already serves the web UI on this port: point at it instead.
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && args.host.is_loopback() => {
            let url = format!("http://127.0.0.1:{port}/?token={token}");
            println!("Beehive web UI (served by the daemon): {url}");
            track_started(&cwd, args.open);
            if args.open {
                bh_core::platform::browser::open_in_browser(&url);
            }
            return Ok(());
        }
        Err(error) => return Err(error).with_context(|| format!("could not listen on port {port}")),
    };
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/?token={token}");
    println!("Beehive web UI: {url}");
    println!("Press Ctrl+C to stop.");
    track_started(&cwd, args.open);
    if args.open {
        bh_core::platform::browser::open_in_browser(&url);
    }
    serve_on(listener, socket_path, cwd, token).await
}

/// Adoption telemetry, delivered by the background worker; the server never waits on it.
fn track_started(cwd: &std::path::Path, open_browser: bool) {
    let agent_dir = crate::config::get_agent_dir();
    let cwd = cwd.to_path_buf();
    tokio::spawn(async move {
        let settings = bh_core::settings::SettingsManager::create(&cwd, &agent_dir);
        if crate::mode::telemetry_disabled(&settings) {
            return;
        }
        let client = bh_core::session_engine::telemetry::build_client(&settings, &agent_dir);
        let mut properties = bh_telemetry::base_properties("interactive");
        properties.set("open_browser", serde_json::json!(open_browser));
        client.track("web ui started", properties);
        let _ = client.shutdown().await;
    });
}

/// Run the `web` command; returns the process exit code.
pub fn run_web_command(args: &[String]) -> i32 {
    let outcome = parse_web_args(args).and_then(|parsed| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(run(parsed))
    });
    match outcome {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Error: {error:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let args: Vec<String> = ["--port", "9000", "--open", "--host", "::1"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            parse_web_args(&args).unwrap(),
            WebArgs {
                host: "::1".parse().unwrap(),
                port: Some(9000),
                open: true,
                daemon_socket: None,
                cwd: None,
            }
        );
    }
}
