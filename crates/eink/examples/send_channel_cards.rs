//! Draws the shipped channel cards on a physically attached panel.
//!
//! The preview sheets prove geometry; only the board proves legibility. Run it
//! with the panel on USB:
//!
//! ```sh
//! cargo run -p brickellstatus-eink --example send_channel_cards
//! cargo run -p brickellstatus-eink --example send_channel_cards -- /dev/cu.usbmodem14B4201
//! ```
//!
//! The panel is read from the board's own banner rather than asked for or
//! assumed, so the frames are always cut for the display that is actually
//! attached.

use std::{env, time::Duration};

use brickellstatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, DeviceBanner,
    PanelModel, RefreshMode, render_channel_card,
    transport::{UsbConfig, UsbTransport, send_frame},
};

/// How long each card stays up, so a person can actually read it.
const DWELL: Duration = Duration::from_secs(6);

fn card(
    channel: ChannelKind,
    urgency: ChannelUrgency,
    title: &str,
    headline: &str,
    detail: &str,
    source: &str,
    age_seconds: u64,
) -> ChannelCard {
    ChannelCard::new(
        channel,
        urgency,
        ChannelAvailability::Current,
        title,
        headline,
        detail,
        // The action line carries the publisher now. Passing the same name to
        // the source tape keeps this sheet honest about what the device shows.
        source,
        ChannelSource::aged(source, age_seconds),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args().nth(1);
    let transport = UsbTransport::new(UsbConfig {
        port: port.clone(),
        ..UsbConfig::default()
    });

    let connection = transport.ensure_connected().await?;
    println!("port    {}", connection.port);

    let banner = connection
        .banner
        .as_deref()
        .map(DeviceBanner::parse)
        .unwrap_or_else(|| DeviceBanner::parse(""));
    if let Some(line) = connection.banner.as_deref() {
        println!("banner  {line}");
    }

    // The board names itself. Falling back to the default would risk sending
    // 250x122 bytes to a 296x128 panel, which the firmware NACKs on size.
    let panel = banner.panel.or(banner.board).unwrap_or_else(|| {
        println!(
            "warning no panel in banner, assuming {:?}",
            PanelModel::default()
        );
        PanelModel::default()
    });
    println!(
        "panel   {} ({}x{})\n",
        panel.label(),
        panel.width(),
        panel.height()
    );

    let cards = [
        (
            "news / publisher on the action line",
            card(
                ChannelKind::News,
                ChannelUrgency::Routine,
                "Headlines",
                "Commission delays vote",
                "Brickell zoning decision pushed to next month",
                "Miami Herald",
                420,
            ),
        ),
        (
            "news / accented Spanish",
            card(
                ChannelKind::News,
                ChannelUrgency::Routine,
                "Venezuela",
                "Sismo de magnitud 4 se sintió en La Guaira",
                "Sin daños reportados en la zona costera",
                "Efecto Cocuyo",
                1_800,
            ),
        ),
        (
            "sports / roster move",
            card(
                ChannelKind::Sports,
                ChannelUrgency::Routine,
                "Miami teams",
                "Dolphins bring back Seth Coleman-Lyles",
                "Roster move / 40 min ago",
                "The Phinsider",
                2_400,
            ),
        ),
        (
            "sports / transaction desk",
            card(
                ChannelKind::Sports,
                ChannelUrgency::Routine,
                "Miami teams",
                "Heat sign Klay Thompson to one-year deal",
                "Roster move / 12 min ago",
                "All U Can Heat",
                720,
            ),
        ),
    ];

    for (index, (label, card)) in cards.iter().enumerate() {
        let frame = render_channel_card(card, panel)?;
        // A full refresh on the first frame clears whatever the board was
        // holding; the rest go out fast, so the sequence reads as a rotation
        // rather than a series of flashes.
        let refresh = if index == 0 {
            RefreshMode::Full
        } else {
            RefreshMode::Fast
        };
        let receipt = send_frame(&transport, &frame, refresh).await?;
        println!("sent    {label}\n        {}", receipt.acknowledgement);
        if index + 1 < cards.len() {
            tokio::time::sleep(DWELL).await;
        }
    }

    transport.disconnect().await;
    println!("\ndone    {} card(s) on {}", cards.len(), panel.label());
    Ok(())
}
