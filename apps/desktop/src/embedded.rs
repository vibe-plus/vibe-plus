//! In-process gateway lifecycle: start vibe-core and poll until ready.

use anyhow::Result;
use std::net::SocketAddr;
use std::time::Duration;
use vibe_core::{config::Config, paths, state::AppState};
use vibe_db::Db;

/// Start the embedded gateway on the given port.
/// This future runs forever (until the runtime is dropped).
pub async fn start(port: u16) -> Result<()> {
    let db_path = paths::db_path()?;
    let cfg_path = paths::config_path()?;
    let mut cfg = Config::load_or_init(&cfg_path)?;
    cfg.server.port = port;
    cfg.save(&cfg_path)?;
    let db = Db::open(&db_path)?;
    let state = AppState::init(db, cfg, port)?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    vibe_core::server::serve(addr, state).await?;
    Ok(())
}

/// Poll /health until we get HTTP 200, or give up after ~30 s.
pub async fn wait_until_ready(port: u16) -> bool {
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if is_responsive(port).await {
            return true;
        }
    }
    false
}

async fn is_responsive(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(mut stream) = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await else {
        return false;
    };
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 64];
    stream
        .read(&mut buf)
        .await
        .is_ok_and(|n| std::str::from_utf8(&buf[..n]).is_ok_and(|s| s.contains(" 200 ")))
}
