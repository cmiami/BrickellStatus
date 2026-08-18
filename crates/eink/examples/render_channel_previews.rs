//! Generates representative generic-channel e-paper design previews.

use std::{fs, path::PathBuf};

use bridgestatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, PanelModel,
    render_channel_card, save_preview_png, save_scaled_preview_png,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("crates/eink/previews"));
    fs::create_dir_all(&output)?;

    let cards = [
        (
            "weather-urgent",
            ChannelCard::new(
                ChannelKind::Weather,
                ChannelUrgency::Urgent,
                ChannelAvailability::Current,
                "Miami / Brickell",
                "Heavy rain in 12 minutes",
                "0.6 in/hr / gusts 31 mph",
                "Take cover by 4:20 PM",
                ChannelSource::aged("Open-Meteo", 42),
            ),
        ),
        (
            "official-critical",
            ChannelCard::new(
                ChannelKind::OfficialAlert,
                ChannelUrgency::Critical,
                ChannelAvailability::Current,
                "Miami-Dade County",
                "Flash flood warning",
                "Until 5:15 PM / move to higher ground",
                "Avoid flooded roads",
                ChannelSource::aged("NWS", 74),
            ),
        ),
    ];

    for (slug, card) in cards {
        for panel in PanelModel::ALL {
            let panel_slug = panel.label().to_lowercase();
            let frame = render_channel_card(&card, panel)?;
            save_preview_png(&frame, output.join(format!("{slug}-{panel_slug}.png")))?;
            save_scaled_preview_png(
                &frame,
                output.join(format!("{slug}-{panel_slug}@4x.png")),
                4,
            )?;
        }
    }

    Ok(())
}
