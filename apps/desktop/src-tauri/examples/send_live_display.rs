//! Refreshes the real collectors, renders the live application snapshot, and
//! sends it to an attached Espressif E213 over USB.

use brickellstatus_desktop_lib::render_live_bridge_frame;
use brickellstatus_eink::{
    RefreshMode,
    transport::{UsbConfig, UsbTransport, send_frame},
};
use brickellstatus_runtime::{RuntimeConfig, RuntimeEngine};
use brickellstatus_storage::Store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let port = arguments.next();
    if arguments.next().is_some() {
        return Err("usage: send_live_display [serial-port]".into());
    }

    let engine = RuntimeEngine::new(Store::in_memory().await?, RuntimeConfig::default()).await?;
    let report = engine.refresh_all().await?;
    let snapshot = engine.get_snapshot().await?;
    let frame = render_live_bridge_frame(&snapshot)?;
    let transport = UsbTransport::new(UsbConfig {
        port,
        ..UsbConfig::default()
    });
    let receipt = send_frame(&transport, &frame, RefreshMode::Full).await?;

    println!(
        "refreshed {} real sources ({} succeeded, {} failed); {} over {:?} (READY observed: {})",
        report.attempted,
        report.succeeded,
        report.failed,
        receipt.acknowledgement,
        receipt.transport,
        receipt.ready_observed
    );
    Ok(())
}
