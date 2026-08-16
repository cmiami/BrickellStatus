//! Renders every panel state onto contact sheets, for design review.
//!
//! The panel is the product, and a panel defect is a visual fact: `WATCH` was
//! drawn as `HATCH` for as long as nobody looked at the pixels. This puts every
//! state on one sheet, including the ones an ordinary run never produces —
//! unconfigured channels, offline sources, and copy long enough to truncate.
//!
//! ```sh
//! cargo run -p bridgestatus-eink --example render_panel_states -- <directory>
//! ```

use std::path::PathBuf;

use bridgestatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, EtaRange,
    Evidence, Freshness, HEIGHT, LiveSnapshot, MonoFrame, RenderConfig, SnapshotState, SpanStatus,
    WIDTH, render_channel_card, render_snapshot,
};
use image::{GrayImage, Luma};

/// Enlargement for the sheet. Nearest-neighbour, so every pixel shown is a
/// pixel the device draws.
const SCALE: u32 = 2;
const GUTTER: u32 = 10;
const COLUMNS: u32 = 3;

fn sheet(frames: &[(String, MonoFrame)], path: PathBuf) {
    let cell_w = u32::from(WIDTH) * SCALE;
    let cell_h = u32::from(HEIGHT) * SCALE;
    let rows = (frames.len() as u32).div_ceil(COLUMNS);
    // Mid grey ground, so the panel's own white edge stays visible against it.
    let mut sheet = GrayImage::from_pixel(
        COLUMNS * cell_w + (COLUMNS + 1) * GUTTER,
        rows * cell_h + (rows + 1) * GUTTER,
        Luma([150]),
    );

    for (index, (name, frame)) in frames.iter().enumerate() {
        let index = index as u32;
        let origin_x = GUTTER + (index % COLUMNS) * (cell_w + GUTTER);
        let origin_y = GUTTER + (index / COLUMNS) * (cell_h + GUTTER);
        for y in 0..cell_h {
            for x in 0..cell_w {
                let black = frame.is_black((x / SCALE) as u16, (y / SCALE) as u16);
                sheet.put_pixel(
                    origin_x + x,
                    origin_y + y,
                    Luma([if black { 0 } else { 255 }]),
                );
            }
        }
        println!("  r{}c{}  {name}", index / COLUMNS, index % COLUMNS);
    }
    sheet.save(&path).expect("contact sheet is writable");
    println!("wrote {}", path.display());
}

fn bridge(state: SnapshotState, eta: Option<EtaRange>, stale: bool) -> MonoFrame {
    let age = if stale { 900 } else { 74 };
    let mut snapshot = LiveSnapshot::brickell(state, Freshness::new("AIS + FL511", age, 180));
    snapshot.eta = eta;
    if state.is_predictive() {
        snapshot.confidence_percent = Some(82);
    }
    snapshot.evidence = vec![
        Evidence::new("outbound vessel", "AIS"),
        Evidence::new("upstream bridge", "FL511"),
    ];
    snapshot.spans = vec![
        SpanStatus::new("2AV", true).opened_at("14:20"),
        SpanStatus::new("1ST", false),
    ];
    render_snapshot(&snapshot, &RenderConfig::default()).expect("fixture snapshot is valid")
}

fn card(
    kind: ChannelKind,
    urgency: ChannelUrgency,
    availability: ChannelAvailability,
    title: &str,
    headline: &str,
    detail: &str,
    action: &str,
) -> MonoFrame {
    render_channel_card(&ChannelCard::new(
        kind,
        urgency,
        availability,
        title,
        headline,
        detail,
        action,
        ChannelSource::aged("Open-Meteo", 42),
    ))
    .expect("fixture card is valid")
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("crates/eink/previews"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("output directory is writable");

    println!("BRIDGE PANEL STATES");
    sheet(
        &[
            ("clear".into(), bridge(SnapshotState::Clear, None, false)),
            (
                "watch + eta range".into(),
                bridge(SnapshotState::Watch, Some(EtaRange::new(6, 9)), false),
            ),
            (
                "likely + eta range".into(),
                bridge(SnapshotState::Likely, Some(EtaRange::new(6, 9)), false),
            ),
            (
                "likely, single-minute eta".into(),
                bridge(SnapshotState::Likely, Some(EtaRange::new(4, 4)), false),
            ),
            (
                "likely, no eta".into(),
                bridge(SnapshotState::Likely, None, false),
            ),
            ("open".into(), bridge(SnapshotState::Open, None, false)),
            (
                "offline".into(),
                bridge(SnapshotState::Offline, None, false),
            ),
            (
                "clear + stale".into(),
                bridge(SnapshotState::Clear, None, true),
            ),
            (
                "watch + stale".into(),
                bridge(SnapshotState::Watch, Some(EtaRange::new(11, 18)), true),
            ),
        ],
        out.join("panel-states-bridge.png"),
    );

    println!("\nCHANNEL CARD STATES");
    sheet(
        &[
            (
                "weather / routine".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Current,
                    "Miami / Brickell",
                    "Clear through evening",
                    "78F / wind 8 mph SE",
                    "Nothing to do",
                ),
            ),
            (
                "weather / advisory".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Advisory,
                    ChannelAvailability::Current,
                    "Miami / Brickell",
                    "Rain likely tonight",
                    "0.2 in/hr after 9 PM",
                    "Umbrella if out late",
                ),
            ),
            (
                "weather / urgent".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Urgent,
                    ChannelAvailability::Current,
                    "Miami / Brickell",
                    "Heavy rain in 12 minutes",
                    "0.6 in/hr / gusts 31 mph",
                    "Take cover by 4:20 PM",
                ),
            ),
            (
                "official / critical".into(),
                card(
                    ChannelKind::OfficialAlert,
                    ChannelUrgency::Critical,
                    ChannelAvailability::Current,
                    "NWS Miami-Dade",
                    "Flash flood warning",
                    "Until 6:15 PM / downtown",
                    "Move to higher ground",
                ),
            ),
            (
                "tropical / urgent".into(),
                card(
                    ChannelKind::Tropical,
                    ChannelUrgency::Urgent,
                    ChannelAvailability::Current,
                    "Atlantic basin",
                    "Track shifted west",
                    "Cat 2 / landfall Thu AM",
                    "Review supplies today",
                ),
            ),
            (
                "news / routine".into(),
                card(
                    ChannelKind::News,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Current,
                    "Miami Herald",
                    "Commission delays vote",
                    "Brickell zoning / 2 hr ago",
                    "Read later",
                ),
            ),
            (
                "earthquake / advisory".into(),
                card(
                    ChannelKind::Earthquake,
                    ChannelUrgency::Advisory,
                    ChannelAvailability::Current,
                    "USGS feed",
                    "M4.1 offshore Cuba",
                    "Depth 22 km / 310 km away",
                    "No action expected",
                ),
            ),
            (
                "markets / routine".into(),
                card(
                    ChannelKind::Markets,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Current,
                    "Watchlist",
                    "SPY -0.8% today",
                    "Close 4:00 PM / vol light",
                    "Nothing to do",
                ),
            ),
            (
                "weather / stale".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Stale,
                    "Miami / Brickell",
                    "Clear through evening",
                    "78F / wind 8 mph SE",
                    "Nothing to do",
                ),
            ),
            (
                "weather / offline".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Offline,
                    "Miami / Brickell",
                    "Last reading 3 hr old",
                    "No link to Open-Meteo",
                    "Check network",
                ),
            ),
            (
                "markets / unavailable".into(),
                card(
                    ChannelKind::Markets,
                    ChannelUrgency::Routine,
                    ChannelAvailability::Unavailable,
                    "Watchlist",
                    "Not configured",
                    "Add symbols in settings",
                    "Open settings",
                ),
            ),
            (
                "truncation stress".into(),
                card(
                    ChannelKind::OfficialAlert,
                    ChannelUrgency::Critical,
                    ChannelAvailability::Stale,
                    "National Weather Service Miami-Dade Broward",
                    "Life threatening flash flooding is occurring right now downtown",
                    "Until 6:15 PM for downtown Miami and all of Brickell Avenue south",
                    "Move to higher ground immediately and do not drive through water",
                ),
            ),
        ],
        out.join("panel-states-channels.png"),
    );
}
