//! Miami River channel geometry for the Brickell Avenue Bridge.
//!
//! Straight-line distance and bearing mislead on a river that bends through
//! ninety degrees, so vessel reasoning uses a channel coordinate instead:
//! every fix projects onto a fixed centerline as `s` — signed meters of
//! channel between the vessel and the Brickell span, positive upriver,
//! negative seaward — plus `offset`, the perpendicular distance off the
//! centerline. A vessel is *in the corridor* when its offset is small; it is
//! *closing* when `|s|` shrinks between fixes. Inbound traffic follows the
//! marked entrance channels rather than approaching radially, so the seaward
//! side continues the same coordinate along the two real approach routes.
//!
//! Trunk waypoints are the FL511-published bascule coordinates (surveyed) plus
//! mouth and upper-river anchors; the approach legs follow the charted
//! Intracoastal/Main Channel routes. Both were checked against live vessel
//! tracks during the 2026-08-17 discovery session (`docs/AIS_DISCOVERY.md`):
//! an outbound tug convoy projected onto the trunk at 28–61 m offset for the
//! whole run, while every moored false-positive candidate fell outside the
//! corridor threshold.

/// Which charted line a fix projected onto.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiverBranch {
    /// The river trunk from Palmer Lake down through the mouth.
    River,
    /// Government Cut / Main Channel / ICW leg approaching from the north.
    NorthApproach,
    /// ICW leg approaching from the Rickenbacker to the south.
    SouthApproach,
}

impl RiverBranch {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::River => "river",
            Self::NorthApproach => "north_approach",
            Self::SouthApproach => "south_approach",
        }
    }

    /// Widest offset still considered "on the channel" for this branch.
    ///
    /// The river threshold must reject the moored pleasure craft observed
    /// 143 m off the span while keeping the convoy that ran the trunk at
    /// ≤ 61 m; the approach channels are wider water, but the moored fleet
    /// off Bayfront begins around 166 m.
    pub(crate) const fn corridor_offset_meters(self) -> f64 {
        match self {
            Self::River => 120.0,
            Self::NorthApproach | Self::SouthApproach => 150.0,
        }
    }
}

/// A position projected into channel coordinates.
#[derive(Clone, Copy, Debug)]
pub struct RiverFix {
    pub branch: RiverBranch,
    /// Signed channel meters to the Brickell span: positive upriver of it,
    /// negative seaward, continuing along the approach branches.
    pub s_meters: f64,
    /// Perpendicular meters off the branch centerline.
    pub offset_meters: f64,
    /// Compass bearing of the channel direction that closes on the bridge at
    /// this point — what a vessel's COG looks like when it is coming.
    pub bridgeward_bearing_degrees: f64,
}

impl RiverFix {
    pub(crate) fn in_corridor(&self) -> bool {
        self.offset_meters <= self.branch.corridor_offset_meters()
    }

    /// Channel distance to the bridge regardless of side.
    pub(crate) fn channel_distance_meters(&self) -> f64 {
        self.s_meters.abs()
    }
}

/// Brickell Avenue Bridge, matching the FL511 target selector.
pub const BRIDGE_LATITUDE: f64 = 25.7699;
pub const BRIDGE_LONGITUDE: f64 = -80.190_05;

/// The corridor model is specific to this bridge; any other configured target
/// falls back to the generic square subscription.
pub(crate) fn is_brickell_target(latitude: f64, longitude: f64) -> bool {
    crate::geo::haversine_meters(latitude, longitude, BRIDGE_LATITUDE, BRIDGE_LONGITUDE) < 1_500.0
}

/// Waypoint: (latitude, longitude, cumulative meters from the first point).
pub type Waypoint = (f64, f64);

/// River trunk, mouth-first so approaches splice on cleanly. The nine bascule
/// coordinates are FL511's own; S Miami Avenue is unpublished there but its
/// span position is charted.
const TRUNK: [Waypoint; 12] = [
    (25.7710, -80.1849),       // mouth at Brickell Point
    (25.7699, -80.190_05),     // Brickell Avenue Bridge (target, s = 0)
    (25.7692, -80.1938),       // S Miami Ave (FL511-invisible)
    (25.768_907, -80.197_552), // SW 2 Ave
    (25.773_038, -80.200_591), // SW 1 St
    (25.774_205, -80.201_287), // W Flagler
    (25.778_307, -80.206_931), // NW 5 St
    (25.782_594, -80.214_716), // NW 12 Ave
    (25.785_884, -80.222_961), // NW 17 Ave
    (25.788_202, -80.231_373), // NW 22 Ave
    (25.792_670, -80.239_650), // NW 27 Ave
    (25.8085, -80.2550),       // Palmer Lake / upper river
];

/// Northern entrance: mouth → ICW leg off Bayfront (surveyed live from an
/// on-channel transit at ≈ −80.1825) → Dodge Island west end → Main Channel →
/// Government Cut jetties.
const NORTH_APPROACH: [Waypoint; 6] = [
    (25.7710, -80.1849),
    (25.7690, -80.1824),
    (25.7663, -80.1830),
    (25.7725, -80.1795),
    (25.7705, -80.1700),
    (25.7635, -80.1330),
];

/// Southern entrance: mouth → ICW toward the Rickenbacker.
const SOUTH_APPROACH: [Waypoint; 4] = [
    (25.7710, -80.1849),
    (25.7620, -80.1845),
    (25.7520, -80.1810),
    (25.7460, -80.1700),
];

/// Distance from the mouth waypoint to the Brickell span along the trunk.
fn mouth_to_bridge_meters() -> f64 {
    crate::geo::haversine_meters(TRUNK[0].0, TRUNK[0].1, TRUNK[1].0, TRUNK[1].1)
}

/// Projects a fix onto the nearest branch centerline.
pub fn project(latitude: f64, longitude: f64) -> RiverFix {
    let mouth_s = -mouth_to_bridge_meters();
    let trunk = project_polyline(latitude, longitude, &TRUNK);
    let north = project_polyline(latitude, longitude, &NORTH_APPROACH);
    let south = project_polyline(latitude, longitude, &SOUTH_APPROACH);

    // Trunk arc-length runs mouth → upriver; shift so Brickell is zero. A
    // vessel upriver of the span closes by heading down-arc; one between the
    // mouth and the span closes by heading up-arc.
    let trunk_s = trunk.arc_meters + mouth_s;
    let river = RiverFix {
        branch: RiverBranch::River,
        s_meters: trunk_s,
        offset_meters: trunk.offset_meters,
        bridgeward_bearing_degrees: segment_bearing(&TRUNK, trunk.segment_index, trunk_s <= 0.0),
    };
    // Approach arc-length runs mouth → seaward; the mouth itself already sits
    // `mouth_s` short of the bridge, and going seaward moves further away, so
    // closing is always down-arc (toward the mouth).
    let north = RiverFix {
        branch: RiverBranch::NorthApproach,
        s_meters: mouth_s - north.arc_meters,
        offset_meters: north.offset_meters,
        bridgeward_bearing_degrees: segment_bearing(&NORTH_APPROACH, north.segment_index, false),
    };
    let south = RiverFix {
        branch: RiverBranch::SouthApproach,
        s_meters: mouth_s - south.arc_meters,
        offset_meters: south.offset_meters,
        bridgeward_bearing_degrees: segment_bearing(&SOUTH_APPROACH, south.segment_index, false),
    };

    let mut best = river;
    for candidate in [north, south] {
        if candidate.offset_meters < best.offset_meters {
            best = candidate;
        }
    }
    best
}

/// Bearing along a polyline segment, up-arc (`forward`) or down-arc.
fn segment_bearing(line: &[Waypoint], segment_index: usize, forward: bool) -> f64 {
    let a = line[segment_index];
    let b = line[segment_index + 1];
    if forward {
        crate::geo::initial_bearing_degrees(a.0, a.1, b.0, b.1)
    } else {
        crate::geo::initial_bearing_degrees(b.0, b.1, a.0, a.1)
    }
}

struct PolylineProjection {
    /// Meters along the polyline from its first point to the projected point.
    arc_meters: f64,
    offset_meters: f64,
    segment_index: usize,
}

fn project_polyline(latitude: f64, longitude: f64, line: &[Waypoint]) -> PolylineProjection {
    // Local tangent-plane approximation, accurate to well under a meter at
    // city scale, which is all a 120 m corridor test needs.
    let meters_per_degree_longitude = 111_320.0 * latitude.to_radians().cos();
    const METERS_PER_DEGREE_LATITUDE: f64 = 110_540.0;

    let mut best = PolylineProjection {
        arc_meters: 0.0,
        offset_meters: f64::INFINITY,
        segment_index: 0,
    };
    let mut cumulative = 0.0;
    for (segment_index, pair) in line.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let ax = (a.1 - longitude) * meters_per_degree_longitude;
        let ay = (a.0 - latitude) * METERS_PER_DEGREE_LATITUDE;
        let bx = (b.1 - longitude) * meters_per_degree_longitude;
        let by = (b.0 - latitude) * METERS_PER_DEGREE_LATITUDE;
        let dx = bx - ax;
        let dy = by - ay;
        let length_squared = (dx * dx + dy * dy).max(1.0);
        let t = (-(ax * dx + ay * dy) / length_squared).clamp(0.0, 1.0);
        let cx = ax + t * dx;
        let cy = ay + t * dy;
        let offset = (cx * cx + cy * cy).sqrt();
        let segment_meters = crate::geo::haversine_meters(a.0, a.1, b.0, b.1);
        if offset < best.offset_meters {
            best = PolylineProjection {
                arc_meters: cumulative + segment_meters * t,
                offset_meters: offset,
                segment_index,
            };
        }
        cumulative += segment_meters;
    }
    best
}

/// What a named point on the corridor is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationKind {
    /// The Brickell span itself.
    Target,
    /// A bascule upstream of the target.
    Bridge,
    /// Where the river meets the bay; both approaches join here.
    Mouth,
    /// A charted turn or channel mark with no bascule on it.
    Waypoint,
}

impl StationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Bridge => "bridge",
            Self::Mouth => "mouth",
            Self::Waypoint => "waypoint",
        }
    }
}

/// A named point on the corridor, for a diagram that names what it draws.
#[derive(Clone, Copy, Debug)]
pub struct Station {
    pub label: &'static str,
    pub kind: StationKind,
    /// FL511 selector key when this station is a bascule the app watches, so a
    /// surface can join it to live bridge state. `None` for a bascule FL511
    /// does not publish, and for every non-bridge station.
    pub bridge_key: Option<&'static str>,
    pub latitude: f64,
    pub longitude: f64,
}

/// One charted branch of the tracked corridor, published for display.
#[derive(Clone, Copy, Debug)]
pub struct CorridorBranch {
    pub id: &'static str,
    pub label: &'static str,
    /// Half-width of the tracked water either side of the centerline.
    pub corridor_offset_meters: f64,
    /// `(latitude, longitude)` waypoints, mouth-first.
    pub centerline: &'static [Waypoint],
    /// Named points along this branch, mouth-first.
    pub stations: &'static [Station],
}

const fn station(
    label: &'static str,
    kind: StationKind,
    bridge_key: Option<&'static str>,
    latitude: f64,
    longitude: f64,
) -> Station {
    Station {
        label,
        kind,
        bridge_key,
        latitude,
        longitude,
    }
}

/// Trunk stations. Bridge keys are FL511's own selector keys, so a station and
/// its live state are the same bridge by construction rather than by a name
/// match that could drift.
const TRUNK_STATIONS: [Station; 11] = [
    station("River mouth", StationKind::Mouth, None, 25.7710, -80.1849),
    station(
        "Brickell Ave",
        StationKind::Target,
        Some("brickell"),
        25.7699,
        -80.190_05,
    ),
    // South Miami Avenue is deliberately absent. FL511 publishes no selector
    // for it, so the app can never say whether it is up or down, and a station
    // drawn with no state reads as "closed" to anyone glancing. The blind spot
    // is recorded in the module docs and in the FL511 selector list; it is not
    // something to draw on a status diagram.
    station(
        "SW 2 Ave",
        StationKind::Bridge,
        Some("sw_2_ave"),
        25.768_907,
        -80.197_552,
    ),
    station(
        "SW 1 St",
        StationKind::Bridge,
        Some("sw_1_st"),
        25.773_038,
        -80.200_591,
    ),
    station(
        "W Flagler",
        StationKind::Bridge,
        Some("w_flagler"),
        25.774_205,
        -80.201_287,
    ),
    station(
        "NW 5 St",
        StationKind::Bridge,
        Some("nw_5_st"),
        25.778_307,
        -80.206_931,
    ),
    station(
        "NW 12 Ave",
        StationKind::Bridge,
        Some("nw_12_ave"),
        25.782_594,
        -80.214_716,
    ),
    station(
        "NW 17 Ave",
        StationKind::Bridge,
        Some("nw_17_ave"),
        25.785_884,
        -80.222_961,
    ),
    station(
        "NW 22 Ave",
        StationKind::Bridge,
        Some("nw_22_ave"),
        25.788_202,
        -80.231_373,
    ),
    station(
        "NW 27 Ave",
        StationKind::Bridge,
        Some("nw_27_ave"),
        25.792_670,
        -80.239_650,
    ),
    station(
        "Palmer Lake",
        StationKind::Waypoint,
        None,
        25.8085,
        -80.2550,
    ),
];

/// North approach marks, named from the charted route the leg follows.
const NORTH_APPROACH_STATIONS: [Station; 6] = [
    station("River mouth", StationKind::Mouth, None, 25.7710, -80.1849),
    station(
        "Bayfront ICW",
        StationKind::Waypoint,
        None,
        25.7690,
        -80.1824,
    ),
    station(
        "Brickell Key",
        StationKind::Waypoint,
        None,
        25.7663,
        -80.1830,
    ),
    station(
        "Dodge Island",
        StationKind::Waypoint,
        None,
        25.7725,
        -80.1795,
    ),
    station(
        "Main Channel",
        StationKind::Waypoint,
        None,
        25.7705,
        -80.1700,
    ),
    station(
        "Government Cut",
        StationKind::Waypoint,
        None,
        25.7635,
        -80.1330,
    ),
];

/// South approach marks along the ICW toward the Rickenbacker.
const SOUTH_APPROACH_STATIONS: [Station; 4] = [
    station("River mouth", StationKind::Mouth, None, 25.7710, -80.1849),
    station("Claughton", StationKind::Waypoint, None, 25.7620, -80.1845),
    station("ICW south", StationKind::Waypoint, None, 25.7520, -80.1810),
    station(
        "Rickenbacker",
        StationKind::Waypoint,
        None,
        25.7460,
        -80.1700,
    ),
];

/// The tracked corridor as geometry a surface can draw.
///
/// Published from the same constants `project` runs on, so the water a map
/// highlights is by construction the water a fix is tested against: the two
/// can never drift into disagreeing about what is being tracked.
pub fn corridor_geometry() -> [CorridorBranch; 3] {
    [
        CorridorBranch {
            id: RiverBranch::River.as_str(),
            label: "Miami River",
            corridor_offset_meters: RiverBranch::River.corridor_offset_meters(),
            centerline: &TRUNK,
            stations: &TRUNK_STATIONS,
        },
        CorridorBranch {
            id: RiverBranch::NorthApproach.as_str(),
            label: "Main Channel approach",
            corridor_offset_meters: RiverBranch::NorthApproach.corridor_offset_meters(),
            centerline: &NORTH_APPROACH,
            stations: &NORTH_APPROACH_STATIONS,
        },
        CorridorBranch {
            id: RiverBranch::SouthApproach.as_str(),
            label: "ICW south approach",
            corridor_offset_meters: RiverBranch::SouthApproach.corridor_offset_meters(),
            centerline: &SOUTH_APPROACH,
            stations: &SOUTH_APPROACH_STATIONS,
        },
    ]
}

/// Bounding boxes tiling the corridor: four slim river tiles plus the two
/// marked entrance channels. Format matches the AISStream subscription:
/// `[[south, west], [north, east]]`.
pub(crate) fn corridor_bounding_boxes() -> Vec<[[f64; 2]; 2]> {
    vec![
        // Lower river: mouth → I-95, including the target span.
        [[25.7660, -80.2020], [25.7760, -80.1840]],
        // Mid river: I-95 → NW 5 St.
        [[25.7730, -80.2130], [25.7870, -80.2000]],
        // Upper-mid river: NW 5 St → NW 22 Ave.
        [[25.7850, -80.2400], [25.8020, -80.2100]],
        // Upper river: NW 22 Ave → Palmer Lake.
        [[25.7990, -80.2600], [25.8100, -80.2380]],
        // North approach: ICW / Main Channel out to Government Cut.
        [[25.7620, -80.1860], [25.7790, -80.1280]],
        // South approach: ICW down to the Rickenbacker.
        [[25.7440, -80.1900], [25.7720, -80.1740]],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_projects_to_zero_and_bascules_land_in_order() {
        let bridge = project(BRIDGE_LATITUDE, BRIDGE_LONGITUDE);
        assert_eq!(bridge.branch, RiverBranch::River);
        assert!(bridge.offset_meters < 1.0);
        assert!(bridge.s_meters.abs() < 1.0);

        // FL511 bascule coordinates must land on the trunk in ascending s.
        let gates = [
            (25.768_907, -80.197_552, 700.0, 850.0),     // SW 2 Ave
            (25.778_307, -80.206_931, 2_000.0, 2_400.0), // NW 5 St
            (25.785_884, -80.222_961, 3_800.0, 4_200.0), // NW 17 Ave
            (25.792_670, -80.239_650, 5_600.0, 6_100.0), // NW 27 Ave
        ];
        let mut previous = 0.0;
        for (latitude, longitude, low, high) in gates {
            let fix = project(latitude, longitude);
            assert_eq!(fix.branch, RiverBranch::River);
            assert!(fix.offset_meters < 5.0);
            assert!(
                fix.s_meters > low && fix.s_meters < high,
                "gate at {latitude},{longitude} projected to s={}",
                fix.s_meters
            );
            assert!(fix.s_meters > previous);
            previous = fix.s_meters;
        }
    }

    #[test]
    fn mid_river_vessel_is_in_corridor_and_off_channel_dock_is_not() {
        // A vessel mid-channel below NW 5 St (synthetic, on the centerline
        // between the Flagler and NW 5 St spans).
        let underway = project(25.7762, -80.2040);
        assert_eq!(underway.branch, RiverBranch::River);
        assert!(underway.in_corridor(), "offset {}", underway.offset_meters);
        assert!(underway.s_meters > 1_500.0 && underway.s_meters < 2_200.0);

        // A dock berth ~140 m south of the span: nearly at the bridge by
        // straight-line distance, outside the corridor by offset.
        let docked = project(25.7687, -80.1895);
        assert!(!docked.in_corridor(), "offset {}", docked.offset_meters);
        assert!(docked.channel_distance_meters() < 250.0);
    }

    #[test]
    fn approach_channels_continue_the_coordinate_seaward() {
        // On the ICW leg just east of the mouth, heading for the entrance.
        let inbound = project(25.7676, -80.1827);
        assert!(matches!(
            inbound.branch,
            RiverBranch::NorthApproach | RiverBranch::SouthApproach
        ));
        assert!(inbound.in_corridor(), "offset {}", inbound.offset_meters);
        assert!(inbound.s_meters < -600.0 && inbound.s_meters > -2_000.0);

        // Out by the Government Cut jetties: far seaward on the north branch.
        let entering = project(25.7640, -80.1360);
        assert_eq!(entering.branch, RiverBranch::NorthApproach);
        assert!(entering.s_meters < -4_500.0);

        // Mid-bay, well off any channel.
        let bay = project(25.7560, -80.1600);
        assert!(!bay.in_corridor(), "offset {}", bay.offset_meters);
    }

    #[test]
    fn published_geometry_is_the_geometry_fixes_are_tested_against() {
        let branches = corridor_geometry();
        assert_eq!(branches.len(), 3);

        // Every published waypoint must project onto its own branch at
        // effectively zero offset. If a centerline were ever published from a
        // separate copy of the constants, a map would highlight water the
        // collector does not actually test against, and this fails.
        for branch in branches {
            assert!(branch.centerline.len() >= 2, "{} has no line", branch.id);
            for &(latitude, longitude) in branch.centerline {
                let fix = project(latitude, longitude);
                assert!(
                    fix.offset_meters < 1.0,
                    "{} waypoint {latitude},{longitude} sits {} m off the corridor",
                    branch.id,
                    fix.offset_meters
                );
                assert!(fix.in_corridor());
            }
            // The published half-width is the threshold `in_corridor` applies.
            let matching = [
                RiverBranch::River,
                RiverBranch::NorthApproach,
                RiverBranch::SouthApproach,
            ]
            .into_iter()
            .find(|candidate| candidate.as_str() == branch.id)
            .expect("published id names a real branch");
            assert_eq!(
                branch.corridor_offset_meters,
                matching.corridor_offset_meters()
            );
        }

        // The published bridge anchor is the target itself.
        let bridge = project(BRIDGE_LATITUDE, BRIDGE_LONGITUDE);
        assert!(bridge.s_meters.abs() < 1.0);
    }

    #[test]
    fn corridor_boxes_are_valid_subscription_geometry() {
        for bounds in corridor_bounding_boxes() {
            let [[south, west], [north, east]] = bounds;
            assert!(south < north && west < east);
            assert!((-90.0..=90.0).contains(&south) && (-90.0..=90.0).contains(&north));
            assert!((-180.0..=180.0).contains(&west) && (-180.0..=180.0).contains(&east));
        }
        assert!(is_brickell_target(BRIDGE_LATITUDE, BRIDGE_LONGITUDE));
        assert!(!is_brickell_target(26.1, -80.12));
    }
}
