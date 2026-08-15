//! Generates the checked-in likely-opening e-paper design preview.

use bridgestatus_eink::{
    EtaRange, Evidence, Freshness, LiveSnapshot, RenderConfig, SnapshotState, render_snapshot,
    save_preview_png, save_scaled_preview_png,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/eink/previews/bridge-likely.png".into());
    let scaled_output = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "crates/eink/previews/bridge-likely@4x.png".into());

    let mut snapshot = LiveSnapshot::brickell(
        SnapshotState::Likely,
        Freshness::new("AIS + FL511", 74, 180),
    );
    snapshot.eta = Some(EtaRange::new(6, 9));
    snapshot.confidence_percent = Some(82);
    snapshot.evidence = vec![
        Evidence::new("outbound vessel", "AIS"),
        Evidence::new("upstream bridge", "FL511"),
    ];

    let frame = render_snapshot(&snapshot, &RenderConfig::default())?;
    save_preview_png(&frame, output)?;
    save_scaled_preview_png(&frame, scaled_output, 4)?;
    Ok(())
}
