//! Client implementation for the `bore` service.

use std::{future::Future, sync::Arc};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::mpsc, time::timeout};
use tracing::{error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::auth::Authenticator;
use crate::shared::{ClientMessage, Delimited, ServerMessage, CONTROL_PORT, NETWORK_TIMEOUT};

/// CLI arguments for the local client tunnel.
#[derive(clap::Args, Debug, Clone, Serialize, Deserialize)]
pub struct LocalArgs {
    /// The local port to expose.
    #[arg(env = "BORE_LOCAL_PORT")]
    pub local_port: u16,

    /// The local host to expose.
    #[arg(short, long, value_name = "HOST", default_value = "localhost")]
    pub local_host: String,

    /// Address of the remote server to expose local ports to.
    #[arg(short, long, env = "BORE_SERVER")]
    pub to: String,

    /// Optional port on the remote server to select.
    #[arg(short, long, default_value_t = 0)]
    pub port: u16,

    /// Optional secret for authentication.
    #[arg(short, long, env = "BORE_SECRET", hide_env_values = true)]
    pub secret: Option<String>,
}

/// Events emitted while a local tunnel is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TunnelEvent {
    /// A log line for the tunnel.
    Log(String),

    /// Tunnel has started and optionally knows its remote port.
    Started {
        /// Remote port assigned by the server, when known.
        remote_port: Option<u16>,
    },

    /// Tunnel stopped cleanly.
    Stopped,

    /// Tunnel failed with an error message.
    Failed(String),
}

/// State structure for the client.
pub struct Client {
    /// Control connection to the server.
    conn: Option<Delimited<TcpStream>>,

    /// Destination address of the server.
    to: String,

    // Local host that is forwarded.
    local_host: String,

    /// Local port that is forwarded.
    local_port: u16,

    /// Port that is publicly available on the remote.
    remote_port: u16,

    /// Optional secret used to authenticate clients.
    auth: Option<Authenticator>,

    /// Optional event sink for web tunnel management.
    event_tx: Option<mpsc::UnboundedSender<TunnelEvent>>,
}

impl Client {
    /// Create a new client.
    pub async fn new(
        local_host: &str,
        local_port: u16,
        to: &str,
        port: u16,
        secret: Option<&str>,
    ) -> Result<Self> {
        Self::new_with_events(local_host, local_port, to, port, secret, None).await
    }

    /// Create a new client and emit tunnel events.
    pub async fn new_with_events(
        local_host: &str,
        local_port: u16,
        to: &str,
        port: u16,
        secret: Option<&str>,
        event_tx: Option<mpsc::UnboundedSender<TunnelEvent>>,
    ) -> Result<Self> {
        let mut stream = Delimited::new(connect_with_timeout(to, CONTROL_PORT).await?);
        let auth = secret.map(Authenticator::new);
        if let Some(auth) = &auth {
            auth.client_handshake(&mut stream).await?;
        }

        stream.send(ClientMessage::Hello(port)).await?;
        let remote_port = match stream.recv_timeout().await? {
            Some(ServerMessage::Hello(remote_port)) => remote_port,
            Some(ServerMessage::Error(message)) => bail!("server error: {message}"),
            Some(ServerMessage::Challenge(_)) => {
                bail!("server requires authentication, but no client secret was provided");
            }
            Some(_) => bail!("unexpected initial non-hello message"),
            None => bail!("unexpected EOF"),
        };
        info!(remote_port, "connected to server");
        info!("listening at {to}:{remote_port}");

        let client = Client {
            conn: Some(stream),
            to: to.to_string(),
            local_host: local_host.to_string(),
            local_port,
            remote_port,
            auth,
            event_tx,
        };
        client.emit_log(format!("connected to {to}:{CONTROL_PORT}"));
        client.emit_log(format!("listening at {to}:{remote_port}"));

        Ok(client)
    }

    /// Returns the port publicly available on the remote.
    pub fn remote_port(&self) -> u16 {
        self.remote_port
    }

    /// Start the client, listening for new connections.
    pub async fn listen(self) -> Result<()> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    /// Start the client, listening for new connections until shutdown resolves.
    pub async fn listen_with_shutdown<S>(mut self, shutdown: S) -> Result<()>
    where
        S: Future<Output = ()>,
    {
        let mut conn = self.conn.take().expect("control connection should exist");
        let this = Arc::new(self);
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    this.emit_log("shutdown requested".to_string());
                    return Ok(());
                }
                message = conn.recv() => {
                    match message? {
                        Some(ServerMessage::Hello(_)) => warn!("unexpected hello"),
                        Some(ServerMessage::Challenge(_)) => warn!("unexpected challenge"),
                        Some(ServerMessage::Heartbeat) => (),
                        Some(ServerMessage::Connection(id)) => {
                            let this = Arc::clone(&this);
                            tokio::spawn(
                                async move {
                                    info!("new connection");
                                    this.emit_log(format!("accepted remote connection {id}"));
                                    match this.handle_connection(id).await {
                                        Ok(_) => info!("connection exited"),
                                        Err(err) => {
                                            this.emit_log(format!("connection {id} exited with error: {err}"));
                                            warn!(%err, "connection exited with error");
                                        }
                                    }
                                }
                                .instrument(info_span!("proxy", %id)),
                            );
                        }
                        Some(ServerMessage::Error(err)) => {
                            this.emit_log(format!("server error: {err}"));
                            error!(%err, "server error");
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    async fn handle_connection(&self, id: Uuid) -> Result<()> {
        let mut remote_conn =
            Delimited::new(connect_with_timeout(&self.to[..], CONTROL_PORT).await?);
        if let Some(auth) = &self.auth {
            auth.client_handshake(&mut remote_conn).await?;
        }
        remote_conn.send(ClientMessage::Accept(id)).await?;
        let mut local_conn = connect_with_timeout(&self.local_host, self.local_port).await?;
        let mut parts = remote_conn.into_parts();
        debug_assert!(parts.write_buf.is_empty(), "framed write buffer not empty");
        local_conn.write_all(&parts.read_buf).await?;
        tokio::io::copy_bidirectional(&mut local_conn, &mut parts.io).await?;
        Ok(())
    }

    fn emit_log(&self, message: String) {
        emit_event(&self.event_tx, TunnelEvent::Log(message));
    }
}

/// Runs a local tunnel with optional shutdown and event reporting.
pub async fn run_local<S>(
    args: LocalArgs,
    shutdown: S,
    event_tx: Option<mpsc::UnboundedSender<TunnelEvent>>,
) -> Result<()>
where
    S: Future<Output = ()>,
{
    emit_event(
        &event_tx,
        TunnelEvent::Log(format!(
            "starting tunnel {}:{} -> {}:{}",
            args.local_host,
            args.local_port,
            args.to,
            if args.port == 0 {
                "auto".to_string()
            } else {
                args.port.to_string()
            }
        )),
    );

    let client = match Client::new_with_events(
        &args.local_host,
        args.local_port,
        &args.to,
        args.port,
        args.secret.as_deref(),
        event_tx.clone(),
    )
    .await
    {
        Ok(client) => client,
        Err(err) => {
            emit_event(&event_tx, TunnelEvent::Failed(err.to_string()));
            return Err(err);
        }
    };

    emit_event(
        &event_tx,
        TunnelEvent::Started {
            remote_port: Some(client.remote_port()),
        },
    );

    if let Err(err) = client.listen_with_shutdown(shutdown).await {
        emit_event(&event_tx, TunnelEvent::Failed(err.to_string()));
        return Err(err);
    }

    emit_event(&event_tx, TunnelEvent::Stopped);
    Ok(())
}

fn emit_event(event_tx: &Option<mpsc::UnboundedSender<TunnelEvent>>, event: TunnelEvent) {
    if let Some(event_tx) = event_tx {
        let _ = event_tx.send(event);
    }
}

async fn connect_with_timeout(to: &str, port: u16) -> Result<TcpStream> {
    match timeout(NETWORK_TIMEOUT, TcpStream::connect((to, port))).await {
        Ok(res) => res,
        Err(err) => Err(err.into()),
    }
    .with_context(|| format!("could not connect to {to}:{port}"))
}
