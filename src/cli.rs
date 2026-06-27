#![allow(missing_docs)]

use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, Result};
use clap::{error::ErrorKind, ArgAction, CommandFactory, Parser, Subcommand};

use crate::{
    client::{run_local, LocalArgs},
    server::Server,
    web::{
        self, SessionInfo, SessionMode, SystemTunnelRole, SystemTunnelSpec, TunnelConfig, WebState,
    },
};

const WEB_RISK_WARNING: &str =
    "Warning: browser access is unauthenticated. Anyone who can reach remote web port can control local loopback tunnels on this machine.";

/// Top-level CLI arguments.
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    disable_version_flag = true,
    after_help = "Examples:\n  bore local 8000 --to bore.pub\n  bore -w\n  bore web --remote --to your-server.com --port 7836 --secret xxx\n  bore home --to your-server.com --secret xxx\n  npx @qinshower/bore web\n  npx @qinshower/bore -- -w"
)]
pub struct Args {
    /// Prints version information.
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    pub version: Option<bool>,

    /// Starts the local web console.
    #[arg(short = 'w', long = "web")]
    pub web: bool,

    /// Address for the local web console.
    #[arg(long = "web-addr", default_value = "127.0.0.1:7836")]
    pub web_addr: SocketAddr,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level command variants.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Starts a local proxy to the remote server.
    Local(LocalArgs),

    /// Starts web console. Prefer this form for `npx`.
    Web(WebArgs),

    /// Starts home bundle with remote web + SSH system tunnels.
    Home(HomeArgs),

    /// Runs remote proxy server.
    Server(ServerArgs),
}

/// Web console CLI arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct WebArgs {
    /// Address for local web console.
    #[arg(long = "web-addr", default_value = "127.0.0.1:7836")]
    pub web_addr: SocketAddr,

    /// Expose web console through remote tunnel on target server.
    #[arg(long, requires = "to")]
    pub remote: bool,

    /// Address of remote server to expose web console to.
    #[arg(short, long)]
    pub to: Option<String>,

    /// Requested remote port for web console tunnel.
    #[arg(short, long, default_value_t = 7836)]
    pub port: u16,

    /// Optional secret for tunnel authentication.
    #[arg(short, long, env = "BORE_SECRET", hide_env_values = true)]
    pub secret: Option<String>,
}

/// Home bundle CLI arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct HomeArgs {
    /// Address of remote server to expose home services to.
    #[arg(short, long)]
    pub to: String,

    /// Optional secret for tunnel authentication.
    #[arg(short, long, env = "BORE_SECRET", hide_env_values = true)]
    pub secret: Option<String>,

    /// Address for local web console.
    #[arg(long = "web-addr", default_value = "127.0.0.1:7836")]
    pub web_addr: SocketAddr,

    /// Requested remote port for web console tunnel.
    #[arg(long = "web-port", default_value_t = 7836)]
    pub web_port: u16,

    /// Local SSH port to forward.
    #[arg(long = "ssh-local-port", default_value_t = 22)]
    pub ssh_local_port: u16,

    /// Requested remote port for SSH tunnel.
    #[arg(long = "ssh-port", default_value_t = 2222)]
    pub ssh_port: u16,
}

/// Server CLI arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct ServerArgs {
    /// Minimum accepted TCP port number.
    #[arg(long, default_value_t = 1024, env = "BORE_MIN_PORT")]
    pub min_port: u16,

    /// Maximum accepted TCP port number.
    #[arg(long, default_value_t = 65535, env = "BORE_MAX_PORT")]
    pub max_port: u16,

    /// Optional secret for authentication.
    #[arg(short, long, env = "BORE_SECRET", hide_env_values = true)]
    pub secret: Option<String>,

    /// IP address to bind to, clients must reach this.
    #[arg(long, default_value = "0.0.0.0")]
    pub bind_addr: IpAddr,

    /// IP address where tunnels will listen on, defaults to --bind-addr.
    #[arg(long)]
    pub bind_tunnels: Option<IpAddr>,
}

/// Validates parsed CLI arguments.
pub fn validate_args(args: &Args) -> std::result::Result<(), clap::Error> {
    match &args.command {
        Some(Command::Web(web_args))
            if web_args.remote && !web_args.web_addr.ip().is_loopback() =>
        {
            Err(Args::command().error(
                ErrorKind::InvalidValue,
                "remote web mode requires --web-addr to bind to loopback",
            ))
        }
        Some(Command::Home(home_args)) if !home_args.web_addr.ip().is_loopback() => {
            Err(Args::command().error(
                ErrorKind::InvalidValue,
                "home mode requires --web-addr to bind to loopback",
            ))
        }
        _ => Ok(()),
    }
}

/// Runs parsed CLI command.
pub async fn run(args: Args) -> Result<()> {
    if let Err(err) = validate_args(&args) {
        err.exit();
    }

    match args.command {
        Some(_) if args.web => {
            Args::command()
                .error(
                    ErrorKind::ArgumentConflict,
                    "--web cannot be used together with a subcommand",
                )
                .exit();
        }
        None if args.web => {
            return run_web_local(args.web_addr).await;
        }
        None => {
            Args::command()
                .error(
                    ErrorKind::MissingSubcommand,
                    "a subcommand is required unless --web is used",
                )
                .exit();
        }
        Some(Command::Local(local_args)) => {
            run_local(
                local_args,
                async {
                    let _ = tokio::signal::ctrl_c().await;
                },
                None,
            )
            .await?;
        }
        Some(Command::Web(web_args)) => {
            if web_args.remote {
                run_web_remote(web_args).await?;
            } else {
                run_web_local(web_args.web_addr).await?;
            }
        }
        Some(Command::Home(home_args)) => {
            run_home(home_args).await?;
        }
        Some(Command::Server(server_args)) => {
            let port_range = server_args.min_port..=server_args.max_port;
            if port_range.is_empty() {
                Args::command()
                    .error(ErrorKind::InvalidValue, "port range is empty")
                    .exit();
            }
            let mut server = Server::new(port_range, server_args.secret.as_deref());
            server.set_bind_addr(server_args.bind_addr);
            server.set_bind_tunnels(server_args.bind_tunnels.unwrap_or(server_args.bind_addr));
            server.listen().await?;
        }
    }

    Ok(())
}

pub async fn run_web_local(web_addr: SocketAddr) -> Result<()> {
    web::serve(web::ServeConfig {
        addr: web_addr,
        session: SessionInfo::local(),
    })
    .await
}

pub async fn run_web_remote(args: WebArgs) -> Result<()> {
    let server = args
        .to
        .clone()
        .ok_or_else(|| anyhow!("--to is required when --remote is used"))?;
    let session = SessionInfo {
        mode: SessionMode::RemoteWeb,
        warnings: vec![WEB_RISK_WARNING.to_string()],
        loopback_only: true,
        web_remote_url: Some(format!("http://{}:{}", server, args.port)),
        ssh_remote_endpoint: None,
    };
    let state = WebState::new(session);
    println!("{WEB_RISK_WARNING}");
    let display_url = format!("http://{}:{}", server, args.port);
    web::run_managed(
        web::ServeConfig {
            addr: args.web_addr,
            session: state.session().await,
        },
        state,
        vec![SystemTunnelSpec {
            role: SystemTunnelRole::WebConsole,
            config: TunnelConfig {
                name: "Web Console".to_string(),
                local_port: args.web_addr.port(),
                to: server,
                port: Some(args.port),
                local_host: "127.0.0.1".to_string(),
                secret: args.secret,
            },
            display_url: Some(display_url),
        }],
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
}

pub async fn run_home(args: HomeArgs) -> Result<()> {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let session = SessionInfo {
        mode: SessionMode::Home,
        warnings: vec![WEB_RISK_WARNING.to_string()],
        loopback_only: true,
        web_remote_url: Some(format!("http://{}:{}", args.to, args.web_port)),
        ssh_remote_endpoint: Some(format!("{}:{}", args.to, args.ssh_port)),
    };
    let state = WebState::new(session);
    println!("{WEB_RISK_WARNING}");
    println!("SSH access: ssh {user}@{} -p {}", args.to, args.ssh_port);
    web::run_managed(
        web::ServeConfig {
            addr: args.web_addr,
            session: state.session().await,
        },
        state,
        vec![
            SystemTunnelSpec {
                role: SystemTunnelRole::WebConsole,
                config: TunnelConfig {
                    name: "Web Console".to_string(),
                    local_port: args.web_addr.port(),
                    to: args.to.clone(),
                    port: Some(args.web_port),
                    local_host: "127.0.0.1".to_string(),
                    secret: args.secret.clone(),
                },
                display_url: Some(format!("http://{}:{}", args.to, args.web_port)),
            },
            SystemTunnelSpec {
                role: SystemTunnelRole::Ssh,
                config: TunnelConfig {
                    name: "SSH".to_string(),
                    local_port: args.ssh_local_port,
                    to: args.to.clone(),
                    port: Some(args.ssh_port),
                    local_host: "127.0.0.1".to_string(),
                    secret: args.secret,
                },
                display_url: Some(format!("{}:{}", args.to, args.ssh_port)),
            },
        ],
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use clap::{error::ErrorKind, CommandFactory, Parser};

    use super::{validate_args, Args, Command};

    #[test]
    fn parse_web_long_flag() {
        let args = Args::try_parse_from(["bore", "--web"]).expect("parse should succeed");
        assert!(args.web);
        assert_eq!(args.web_addr.to_string(), "127.0.0.1:7836");
        assert!(args.command.is_none());
    }

    #[test]
    fn parse_web_short_flag() {
        let args = Args::try_parse_from(["bore", "-w"]).expect("parse should succeed");
        assert!(args.web);
    }

    #[test]
    fn parse_web_addr() {
        let args = Args::try_parse_from(["bore", "--web", "--web-addr", "127.0.0.1:9000"])
            .expect("parse should succeed");
        assert_eq!(args.web_addr.to_string(), "127.0.0.1:9000");
    }

    #[test]
    fn parse_existing_local_command() {
        let args = Args::try_parse_from(["bore", "local", "8000", "--to", "bore.pub"])
            .expect("parse should succeed");
        assert!(!args.web);
        assert!(args.command.is_some());
    }

    #[test]
    fn parse_web_subcommand() {
        let args = Args::try_parse_from(["bore", "web", "--web-addr", "127.0.0.1:9000"])
            .expect("parse should succeed");
        assert!(!args.web);
        assert!(matches!(args.command, Some(Command::Web(_))));
    }

    #[test]
    fn parse_remote_web_subcommand() {
        let args = Args::try_parse_from(["bore", "web", "--remote", "--to", "host"])
            .expect("parse should succeed");
        let Some(Command::Web(web)) = args.command else {
            panic!("expected web command");
        };
        assert!(web.remote);
        assert_eq!(web.to.as_deref(), Some("host"));
    }

    #[test]
    fn parse_home_subcommand() {
        let args =
            Args::try_parse_from(["bore", "home", "--to", "host"]).expect("parse should succeed");
        let Some(Command::Home(home)) = args.command else {
            panic!("expected home command");
        };
        assert_eq!(home.to, "host");
        assert_eq!(home.web_port, 7836);
        assert_eq!(home.ssh_port, 2222);
    }

    #[test]
    fn parse_web_remote_missing_to_fails() {
        let err = Args::try_parse_from(["bore", "web", "--remote"]).expect_err("parse should fail");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn validate_remote_web_requires_loopback_addr() {
        let args = Args::try_parse_from([
            "bore",
            "web",
            "--remote",
            "--to",
            "host",
            "--web-addr",
            "0.0.0.0:7836",
        ])
        .expect("parse should succeed");
        let err = validate_args(&args).expect_err("validation should fail");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn parse_existing_server_command() {
        let args = Args::try_parse_from(["bore", "server", "--bind-addr", "0.0.0.0"])
            .expect("parse should succeed");
        assert!(!args.web);
        assert!(args.command.is_some());
    }

    #[test]
    fn parse_version_short_flag() {
        let err = Args::try_parse_from(["bore", "-v"]).expect_err("version should exit");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn help_mentions_web_remote_and_home_usage() {
        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("-w, --web"));
        assert!(help.contains("bore web --remote --to your-server.com --port 7836 --secret xxx"));
        assert!(help.contains("bore home --to your-server.com --secret xxx"));
        assert!(help.contains("npx @qinshower/bore -- -w"));
    }
}
