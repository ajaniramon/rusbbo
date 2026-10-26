//! TCP listener and per-connection tasks (Java: `NettyServer`, `ConnectionHandler`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use havana_protocol::Decoder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::messages::{self, outgoing};
use crate::session::{Outgoing, Session};
use crate::world::World;

pub async fn serve(world: Arc<World>, bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    info!("Shockwave game server is listening on {bind}");
    let ids = AtomicU64::new(0);
    loop {
        let (stream, peer) = listener.accept().await?;
        let id = ids.fetch_add(1, Ordering::Relaxed);
        let world = world.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(world, stream, id).await {
                debug!(conn = id, %peer, "connection ended: {e:#}");
            }
        });
    }
}

async fn connection(world: Arc<World>, stream: TcpStream, id: u64) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = Arc::new(Session::new(id, tx));

    let mut writer_task = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            match out {
                Outgoing::Frame(bytes) => {
                    if writer.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Outgoing::Close => break,
            }
        }
        let _ = writer.shutdown().await;
    });

    session.send(outgoing::handshake::hello());

    let mut decoder = Decoder::default();
    let mut buf = vec![0u8; 8192];
    let mut writer_done = false;
    let result = loop {
        tokio::select! {
            read = reader.read(&mut buf) => {
                let n = match read {
                    Ok(0) => break Ok(()),
                    Ok(n) => n,
                    Err(e) => break Err(e.into()),
                };
                decoder.push(&buf[..n]);
                while let Some(msg) = decoder.next_client() {
                    messages::handle(&world, &session, msg);
                }
            }
            // The server closed the connection (kick): stop reading too.
            _ = &mut writer_task, if !writer_done => {
                writer_done = true;
                break Ok(());
            }
        }
    };

    crate::game::player::dispose(&world, &session);
    session.close();
    if !writer_done {
        if let Err(e) = writer_task.await {
            warn!(conn = id, "writer task: {e}");
        }
    }
    result
}
