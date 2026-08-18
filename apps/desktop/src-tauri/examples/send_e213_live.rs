//! Sends the current persisted runtime snapshot to an attached E213 over USB.

use std::{env, path::PathBuf, sync::Arc};

use brickellstatus_desktop_lib::render_live_bridge_frame;
use brickellstatus_eink::{
    RefreshMode,
    transport::{UsbConfig, UsbTransport, send_frame},
};
use brickellstatus_runtime::{CredentialFreeCollectorFactory, RuntimeConfig, RuntimeEngine};
use brickellstatus_storage::Store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let mut arguments = env::args_os().skip(1);
    let database = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: send_e213_live <sqlite-path> <serial-port>")?;
    let port = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("usage: send_e213_live <sqlite-path> <serial-port>")?;
    if arguments.next().is_some() {
        return Err("usage: send_e213_live <sqlite-path> <serial-port>".into());
    }

    let ais_key = env::var("AISSTREAM_API_KEY").map_err(
        |_| "AISSTREAM_API_KEY must be available so the saved secret flag is not altered",
    )?;
    let factory = CredentialFreeCollectorFactory::new(
        "BrickellStatus E213 live proof (+https://github.com/cmiami/BrickellStatus)",
    )?
    .with_aisstream_key(Some(ais_key))?;
    let engine = RuntimeEngine::with_factory(
        Store::open(database).await?,
        RuntimeConfig::default(),
        Arc::new(factory),
    )
    .await?;
    let snapshot = engine.get_snapshot().await?;
    let frame = render_live_bridge_frame(&snapshot)?;

    let transport = UsbTransport::new(UsbConfig {
        port: Some(port),
        ..UsbConfig::default()
    });
    let connection = transport.ensure_connected().await?;
    println!(
        "opened {} · READY INK1 observed: {}",
        connection.port, connection.ready_observed
    );
    let receipt = send_frame(&transport, &frame, RefreshMode::Full).await?;
    println!(
        "{} · transport {:?} · READY observed: {}",
        receipt.acknowledgement, receipt.transport, receipt.ready_observed
    );
    Ok(())
}
