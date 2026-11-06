use std::{env, net::SocketAddr};

use anyhow::{Context, ensure};
use mysql_replica_lag_lab::{api, config::Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let addr: SocketAddr = env::var("DASHBOARD_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()
        .context("invalid DASHBOARD_BIND_ADDR")?;
    ensure!(
        addr.ip().is_loopback(),
        "DASHBOARD_BIND_ADDR must be a loopback address"
    );
    let app = api::router(Config::from_env()?)?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind dashboard API")?;
    println!(
        "Dashboard API listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app)
        .await
        .context("dashboard HTTP server stopped")
}
