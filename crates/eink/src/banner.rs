//! What a board says about itself when it boots.
//!
//! The firmware speaks one line on the wire:
//!
//! ```text
//! READY INK1 296x128 4736 9f3c2ab E290 26B4 FW4 BAT4012
//! ```
//!
//! Geometry, payload size, exact source build, board identity, and an orderable
//! firmware version. That line is the whole of the host's device
//! identification: nobody is asked which board is plugged in, and no preference
//! records it, because the board is the only thing that actually knows.
//!
//! A build can also land on a board it cannot drive — the display library binds
//! one pinout per image, and a fresh board is flashed before anything knows
//! which one it is. That case is spoken plainly:
//!
//! ```text
//! READY INK1 0x0 0 9f3c2ab E290 26B4 FW2 MISMATCH
//! ```
//!
//! No geometry, because there is nothing this image can correctly draw; the
//! board name, because that is what the app needs in order to write the build
//! that belongs there.
//!
//! Older firmware stops after the payload size, and firmware built without git
//! history stamps `unknown` for the build. Both are absences, never
//! disagreements — a board that cannot name its build must not be reported as
//! running the wrong one.

use crate::{PanelHardware, PanelModel};

/// What the firmware banner told us about the attached board.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceBanner {
    /// Whether `READY INK1` was seen at all.
    pub saw_ready: bool,
    /// Panel this firmware can draw for right now, from the geometry it
    /// announced. Absent on a build sitting on the wrong board.
    pub panel: Option<PanelModel>,
    /// Board the firmware's probe actually found, whether or not it can drive
    /// it, reduced to framebuffer geometry for rendering and legacy callers.
    pub board: Option<PanelModel>,
    /// Exact board family around that panel.
    ///
    /// The Wireless Paper and Vision Master E213 share a 250×122 framebuffer,
    /// but flashing one board's pinout onto the other is not safe. Firmware
    /// selection therefore uses this identity instead of geometry alone.
    pub hardware: Option<PanelHardware>,
    /// Whether the firmware reported that it cannot drive the board it is on.
    pub mismatch: bool,
    /// Build identity, when the firmware is new enough to report one.
    pub build: Option<String>,
    /// Monotonic firmware release, independent of the exact source build.
    ///
    /// Git hashes identify bytes but have no ordering. This number is what the
    /// app uses to call a bundled image newer or a device image newer.
    pub firmware_version: Option<u32>,
    /// A version token was present but could not be interpreted.
    ///
    /// Kept distinct from legacy firmware that predates version reporting: a
    /// malformed current identity is incompatible, not merely old.
    pub version_malformed: bool,
    /// Four hex characters naming this individual board, when the firmware is
    /// new enough to report them. Older firmware omits it, which is an absence
    /// rather than a disagreement: the board is still usable, it just cannot be
    /// told apart from another one of the same model.
    pub board_id: Option<String>,
    /// Measured battery voltage in millivolts, when current firmware reported
    /// a plausible reading. No token, a malformed token, or an out-of-range
    /// reading is unknown rather than zero.
    pub battery_millivolts: Option<u16>,
    /// Firmware's hysteretic low-voltage state. Present only alongside a valid
    /// battery voltage, so legacy firmware and invalid measurements remain
    /// unknown rather than silently becoming "battery okay."
    pub low_battery: Option<bool>,
}

/// What the firmware stamps into its banner when it cannot name its own source
/// revision. Treated as "no build id" on both sides of the comparison.
pub const UNKNOWN_BUILD: &str = "unknown";

impl DeviceBanner {
    /// Reads a banner line emitted by the device.
    pub fn parse(line: &str) -> Self {
        let trimmed = line.trim();
        if !(trimmed.contains("READY INK1") || trimmed == "READY") {
            return Self::default();
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        // `READY INK1 <w>x<h> <payload> <build> <board> <id> FW<n>
        // [MISMATCH] [BAT<mV> [LOWBAT]]`. What the firmware can draw is read
        // from the geometry rather than from the name, because the geometry is
        // what every frame must match.
        let panel = tokens.get(2).and_then(|token| parse_dimensions(token));
        let build = tokens
            .get(4)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| !value.eq_ignore_ascii_case(UNKNOWN_BUILD))
            .map(str::to_owned);
        let hardware = tokens
            .get(5)
            .and_then(|name| PanelHardware::from_label(name));
        let board = hardware
            .map(PanelHardware::panel)
            // Firmware old enough to omit the board name is firmware from
            // before there was a second board, so what it draws is what it is.
            .or(panel);
        // Scanned rather than read at a fixed index. Firmware that reports a
        // board id pushes MISMATCH one place to the right, and firmware that
        // predates the id does not; a positional read would silently stop
        // seeing the mismatch on exactly the boards that report it.
        let mismatch = tokens
            .iter()
            .skip(5)
            .any(|token| token.eq_ignore_ascii_case("MISMATCH"));
        let board_id = tokens
            .get(6)
            .map(|value| value.trim())
            .filter(|value| !value.eq_ignore_ascii_case("MISMATCH"))
            .and_then(valid_board_id);
        let version_token = tokens
            .iter()
            .skip(6)
            .find(|token| token.to_ascii_uppercase().starts_with("FW"));
        let firmware_version = version_token
            .and_then(|token| token.get(2..))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|version| *version > 0);
        let version_malformed = version_token.is_some() && firmware_version.is_none();
        let (battery_millivolts, low_battery) = parse_battery_telemetry(trimmed);
        Self {
            saw_ready: true,
            panel,
            board,
            hardware,
            mismatch,
            build,
            firmware_version,
            version_malformed,
            board_id,
            battery_millivolts,
            low_battery,
        }
    }
}

/// Battery tokens shared by READY banners and ACK receipts.
///
/// The range rejects the phantom low divider reading produced when no battery
/// is attached, along with corrupt values. `LOWBAT` (READY) or `LOW` (compact
/// ACK) without a valid voltage is not actionable and therefore remains
/// unknown.
pub(crate) fn parse_battery_telemetry(line: &str) -> (Option<u16>, Option<bool>) {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let battery_millivolts = tokens
        .iter()
        .find(|token| {
            token
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("BAT"))
        })
        .and_then(|token| token.get(3..))
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|millivolts| (2500..=5000).contains(millivolts));
    let low_battery = battery_millivolts.map(|_| {
        tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("LOWBAT") || token.eq_ignore_ascii_case("LOW"))
    });
    (battery_millivolts, low_battery)
}

/// Stable suffix printed by current firmware: exactly two MAC octets.
fn valid_board_id(value: &str) -> Option<String> {
    (value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_uppercase())
}

/// Reads a `250x122` geometry token into the panel it describes.
fn parse_dimensions(token: &str) -> Option<PanelModel> {
    let (width, height) = token.split_once(['x', 'X'])?;
    PanelModel::from_dimensions(width.parse().ok()?, height.parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_banner_names_the_panel_by_its_geometry() {
        let banner = DeviceBanner::parse("READY INK1 296x128 4736 9f3c2ab E290");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, Some(PanelModel::E290));
        assert_eq!(banner.board, Some(PanelModel::E290));
        assert!(!banner.mismatch);
        assert_eq!(banner.build.as_deref(), Some("9f3c2ab"));
    }

    /// A build on the wrong board draws nothing and says which board it is on,
    /// which is the whole of what the app needs to put that right.
    #[test]
    fn a_banner_names_the_individual_board() {
        let banner =
            DeviceBanner::parse("READY INK1 296x128 4736 9f3c2ab E290 26B4 FW3 BAT3375 LOWBAT");
        assert_eq!(banner.board_id.as_deref(), Some("26B4"));
        assert_eq!(banner.hardware, Some(PanelHardware::VisionMasterE290));
        assert_eq!(banner.firmware_version, Some(3));
        assert!(!banner.version_malformed);
        assert!(!banner.mismatch);
        assert_eq!(banner.battery_millivolts, Some(3375));
        assert_eq!(banner.low_battery, Some(true));
    }

    #[test]
    fn wireless_paper_keeps_its_hardware_identity_at_e213_geometry() {
        let banner = DeviceBanner::parse("READY INK1 250x122 3904 abc1234 WPAPER 26B4 FW6 BAT4012");
        assert_eq!(banner.panel, Some(PanelModel::E213));
        assert_eq!(banner.board, Some(PanelModel::E213));
        assert_eq!(banner.hardware, Some(PanelHardware::WirelessPaper));
    }

    #[test]
    fn missing_or_invalid_battery_data_stays_unknown() {
        let legacy = DeviceBanner::parse("READY INK1 250x122 3904 abc1234 E213 26B4 FW2");
        assert_eq!(legacy.battery_millivolts, None);
        assert_eq!(legacy.low_battery, None);

        for line in [
            "READY INK1 250x122 3904 abc1234 E213 26B4 FW3 BATnone LOWBAT",
            "READY INK1 250x122 3904 abc1234 E213 26B4 FW3 BAT2204 LOWBAT",
            "READY INK1 250x122 3904 abc1234 E213 26B4 FW3 LOWBAT",
        ] {
            let banner = DeviceBanner::parse(line);
            assert_eq!(banner.battery_millivolts, None, "accepted {line:?}");
            assert_eq!(banner.low_battery, None, "accepted {line:?}");
        }
    }

    #[test]
    fn malformed_and_legacy_versions_remain_distinct() {
        let malformed = DeviceBanner::parse("READY INK1 296x128 4736 9f3c2ab E290 26B4 FWnext");
        assert_eq!(malformed.firmware_version, None);
        assert!(malformed.version_malformed);

        let legacy = DeviceBanner::parse("READY INK1 296x128 4736 9f3c2ab E290 26B4");
        assert_eq!(legacy.firmware_version, None);
        assert!(!legacy.version_malformed);
    }

    #[test]
    fn a_mismatch_is_still_seen_when_an_id_precedes_it() {
        let banner = DeviceBanner::parse("READY INK1 0x0 0 9f3c2ab E290 26B4 MISMATCH");
        assert!(banner.mismatch);
        assert_eq!(banner.board_id.as_deref(), Some("26B4"));
    }

    #[test]
    fn firmware_without_an_id_reports_none_rather_than_mismatch_text() {
        let banner = DeviceBanner::parse("READY INK1 0x0 0 9f3c2ab E290 MISMATCH");
        assert!(banner.mismatch);
        assert_eq!(banner.board_id, None);
    }

    #[test]
    fn malformed_board_ids_are_never_used_as_device_identity() {
        for value in ["26B", "26B4FF", "26-Z", "panel"] {
            let banner =
                DeviceBanner::parse(&format!("READY INK1 250x122 3904 abc1234 E213 {value}"));
            assert_eq!(banner.board_id, None, "accepted malformed id {value:?}");
        }
        assert_eq!(
            DeviceBanner::parse("READY INK1 250x122 3904 abc1234 E213 26b4")
                .board_id
                .as_deref(),
            Some("26B4")
        );
    }

    #[test]
    fn a_mismatched_build_names_the_board_it_landed_on() {
        let banner = DeviceBanner::parse("READY INK1 0x0 0 9f3c2ab E290 MISMATCH");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, None, "nothing can be drawn on it yet");
        assert_eq!(banner.board, Some(PanelModel::E290));
        assert!(banner.mismatch);
        assert_eq!(banner.build.as_deref(), Some("9f3c2ab"));
    }

    /// A board with no panel wired at all is not an E213 by default.
    #[test]
    fn a_board_with_no_panel_names_none() {
        let banner = DeviceBanner::parse("READY INK1 0x0 0 9f3c2ab NONE MISMATCH");
        assert_eq!(banner.panel, None);
        assert_eq!(banner.board, None);
        assert!(banner.mismatch);
    }

    /// Boards in service run firmware from before there was a board name to
    /// print. What such a board draws is what it is.
    #[test]
    fn the_original_panel_still_parses_exactly_as_it_did() {
        let banner = DeviceBanner::parse("READY INK1 250x122 3904 abc1234");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, Some(PanelModel::E213));
        assert_eq!(banner.board, Some(PanelModel::E213));
        assert!(!banner.mismatch);
        assert_eq!(banner.build.as_deref(), Some("abc1234"));
    }

    /// Firmware old enough to omit its build id is working firmware, and its
    /// geometry is still usable.
    #[test]
    fn an_older_banner_without_a_build_still_names_its_panel() {
        let banner = DeviceBanner::parse("READY INK1 250x122 3904");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, Some(PanelModel::E213));
        assert_eq!(banner.build, None);
    }

    #[test]
    fn a_build_that_cannot_be_named_is_absent_rather_than_wrong() {
        assert_eq!(
            DeviceBanner::parse("READY INK1 250x122 3904 unknown").build,
            None
        );
    }

    /// A geometry this host cannot draw is not silently rounded to a panel it
    /// can: the host would then send frames the board is bound to refuse.
    #[test]
    fn an_unknown_geometry_leaves_the_panel_unnamed() {
        let banner = DeviceBanner::parse("READY INK1 400x300 15000 abc1234");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, None);
    }

    #[test]
    fn a_bare_ready_is_a_board_that_said_almost_nothing() {
        let banner = DeviceBanner::parse("READY");
        assert!(banner.saw_ready);
        assert_eq!(banner.panel, None);
        assert_eq!(banner.build, None);
    }

    #[test]
    fn unrelated_serial_chatter_is_not_a_banner() {
        assert_eq!(
            DeviceBanner::parse("ets Jul 29 2019 rst:0x1"),
            DeviceBanner::default()
        );
    }
}
