//! The always-on hub will expose the same runtime over authenticated Axum SSE.
//! It is intentionally inert until LAN/authentication configuration is ready.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("PuenteGonorrea hub scaffold ready; no listener is enabled by default");
    Ok(())
}
