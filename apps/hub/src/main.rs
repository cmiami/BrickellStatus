//! The always-on hub will expose the same runtime over authenticated Axum SSE.
//! It is intentionally inert until LAN/authentication configuration is ready.

fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("BrickellStatus hub scaffold ready; no listener is enabled by default");
}
