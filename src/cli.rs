#![allow(missing_docs)]

use std::future::pending;
use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use clap::{error::ErrorKind, ArgAction, CommandFactory, Parser, Subcommand};

use crate::{
    client::{run_local, LocalArgs},
    server::Server,
    web,
};

/// Top-level CLI arguments.
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    disable_version_flag = true,
    after_help = "Examples:\n  bore local 8000 --to bore.pub\n  bore -w\n  bore web --web-addr 127.0.0.1:9000\n  npx @qinshower/bore web\n  npx @qinshower/bore -- -w"
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

    /// Starts the local web console. Prefer this form for `npx`.
    Web(WebArgs),

    /// Runs the remote proxy server.
    Server(ServerArgs),
}

/// Web console CLI arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct WebArgs {
    /// Address for the local web console.
    #[arg(long = "web-addr", default_value = "127.0.0.1:7836")]
    pub web_addr: SocketAddr,
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

/// Runs the parsed CLI command.
pub async fn run(args: Args) -> Result<()> {
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
            return web::serve(args.web_addr).await;
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
            run_local(local_args, pending::<()>(), None).await?;
        }
        Some(Command::Web(web_args)) => {
            web::serve(web_args.web_addr).await?;
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::Args;

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
        assert!(matches!(args.command, Some(super::Command::Web(_))));
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
    fn help_mentions_web_and_npx_usage() {
        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("-w, --web"));
        assert!(help.contains("bore web --web-addr 127.0.0.1:9000"));
        assert!(help.contains("npx @qinshower/bore -- -w"));
    }
}
