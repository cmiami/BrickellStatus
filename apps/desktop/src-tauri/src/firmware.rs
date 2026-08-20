//! Bundled panel firmware and the USB flash workflow.
//!
//! The app ships the firmware it expects the device to run, so a user who plugs
//! in a board can bring it up without a toolchain, and without being asked what
//! they plugged in.
//!
//! The display library binds one board's pinout and controller per image, so
//! `platformio.ini` builds one environment per panel. Nobody chooses between
//! them: every build runs the same probe at boot and reports the board it
//! actually found, so
//!
//! - a board already running this firmware names itself, and the matching build
//!   is written;
//! - a board that has never been flashed is written the most likely build, and
//!   if the probe then reports a different board the app writes the right one
//!   without asking again.
//!
//! The single exception is the E213's panel revision. Its two revisions are the
//! same ESP32-S3, the same USB identifiers, the same six pins and the same BUSY
//! line — only the controller behind the glass differs, and it cannot be read
//! back. That one stays a remembered answer, arrived at by flashing a build and
//! asking whether the screen is readable, never by asking someone to identify
//! hardware sealed inside a case.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use brickellstatus_eink::PanelModel;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bootloader offset for the ESP32-S3. Note this is *not* the 0x1000 used by
/// the original ESP32 and the S2; flashing at the wrong offset produces a board
/// that enumerates over USB and never boots.
pub const BOOTLOADER_OFFSET: u32 = 0x0;
/// Partition table offset, fixed by the ROM bootloader.
pub const PARTITION_TABLE_OFFSET: u32 = 0x8000;
/// OTA data offset. The shipped partition table declares `otadata` here plus
/// `app0`/`app1` slots, so a flash that omits this image leaves whatever OTA
/// selection was there before: a board previously running from `app1` keeps
/// booting `app1`, which this flash never wrote.
pub const OTA_DATA_OFFSET: u32 = 0xe000;
/// First application slot, `app0` in the shipped table.
pub const APP_OFFSET: u32 = 0x10000;

/// Offsets every variant must supply for the device to boot what was flashed.
const REQUIRED_OFFSETS: [u32; 4] = [
    BOOTLOADER_OFFSET,
    PARTITION_TABLE_OFFSET,
    OTA_DATA_OFFSET,
    APP_OFFSET,
];

/// Chip this bundle targets. Guards against a manifest built for another board
/// being flashed onto one of these.
pub const EXPECTED_CHIP: &str = "esp32s3";

/// Manifest revision this app reads. Version 2 names the board each build
/// drives, which is what lets the app pick one without asking.
pub const MANIFEST_SCHEMA_VERSION: u32 = 2;

/// Which E213 panel controller a build drives.
///
/// Only meaningful on the E213, whose two revisions are electrically identical.
/// Every other board has exactly one build and nothing to choose.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelRevision {
    /// Original Vision Master E213 panel.
    Original,
    /// Vision Master E213 revision 1.1 panel.
    V11,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareImageSpec {
    /// Flash offset, written as hex in the manifest for legibility.
    pub offset: String,
    /// File name, resolved relative to the variant directory.
    pub file: String,
}

impl FirmwareImageSpec {
    fn parsed_offset(&self) -> Result<u32, FirmwareError> {
        let raw = self.offset.trim();
        let parsed = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .map(|hex| u32::from_str_radix(hex, 16))
            .unwrap_or_else(|| raw.parse::<u32>())
            .map_err(|_| FirmwareError::Offset {
                value: self.offset.clone(),
            })?;
        Ok(parsed)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareVariantSpec {
    /// PlatformIO environment name, e.g. `vision-master-e213-v11`.
    pub id: String,
    /// Operator-facing name.
    pub label: String,
    /// Board this build drives, which is what the device's own probe reports
    /// back and therefore what a flash decision is made from.
    pub panel: PanelModel,
    /// Which E213 controller this build carries, on the board that has two.
    #[serde(default)]
    pub panel_revision: Option<PanelRevision>,
    pub images: Vec<FirmwareImageSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareManifest {
    pub schema_version: u32,
    pub chip: String,
    /// Firmware source revision, so a support question can be answered without
    /// guessing which build is on the device.
    #[serde(default)]
    pub source_revision: Option<String>,
    pub variants: Vec<FirmwareVariantSpec>,
}

/// One image resolved to bytes, ready to hand to the flasher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlashSegment {
    pub offset: u32,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A validated, flashable build.
#[derive(Clone, Debug)]
pub struct FirmwareVariant {
    pub id: String,
    pub label: String,
    pub panel: PanelModel,
    pub panel_revision: Option<PanelRevision>,
    pub segments: Vec<FlashSegment>,
}

impl FirmwareVariant {
    /// Total bytes written to the device, for progress reporting.
    pub fn total_bytes(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.bytes.len())
            .sum()
    }
}

/// Every firmware build shipped inside the app.
#[derive(Clone, Debug)]
pub struct FirmwareBundle {
    pub source_revision: Option<String>,
    variants: Vec<FirmwareVariant>,
}

impl FirmwareBundle {
    /// Reads and validates `manifest.json` and every image it references.
    ///
    /// Validation is deliberately strict and happens up front rather than
    /// mid-flash: a device interrupted between the bootloader and the app is
    /// bricked until someone reflashes it over USB, so a malformed bundle must
    /// fail before the first byte is written.
    pub fn load(root: &Path) -> Result<Self, FirmwareError> {
        let manifest_path = root.join("manifest.json");
        let raw = fs::read_to_string(&manifest_path).map_err(|error| FirmwareError::Read {
            path: manifest_path.clone(),
            detail: error.to_string(),
        })?;
        let manifest: FirmwareManifest =
            serde_json::from_str(&raw).map_err(|error| FirmwareError::Manifest {
                detail: error.to_string(),
            })?;
        Self::from_manifest(manifest, root)
    }

    fn from_manifest(manifest: FirmwareManifest, root: &Path) -> Result<Self, FirmwareError> {
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(FirmwareError::SchemaVersion(manifest.schema_version));
        }
        if !manifest.chip.eq_ignore_ascii_case(EXPECTED_CHIP) {
            return Err(FirmwareError::Chip {
                found: manifest.chip,
            });
        }
        if manifest.variants.is_empty() {
            return Err(FirmwareError::NoVariants);
        }

        let mut seen_ids = BTreeMap::new();
        let mut variants = Vec::with_capacity(manifest.variants.len());
        for spec in manifest.variants {
            if seen_ids.insert(spec.id.clone(), ()).is_some() {
                return Err(FirmwareError::DuplicateVariant(spec.id));
            }
            variants.push(resolve_variant(spec, root)?);
        }

        Ok(Self {
            source_revision: manifest.source_revision,
            variants,
        })
    }

    pub fn variants(&self) -> &[FirmwareVariant] {
        &self.variants
    }

    pub fn variant(&self, id: &str) -> Option<&FirmwareVariant> {
        self.variants.iter().find(|variant| variant.id == id)
    }

    /// The build to write to a board.
    ///
    /// `board` is what the device reported about itself, so on everything but a
    /// virgin board this is a fact rather than a guess. `revision` is the
    /// remembered answer to the one question hardware cannot settle, and only
    /// the E213 has more than one build for it to choose between.
    pub fn for_board(
        &self,
        board: PanelModel,
        revision: Option<PanelRevision>,
    ) -> Option<&FirmwareVariant> {
        let for_board = || {
            self.variants
                .iter()
                .filter(|variant| variant.panel == board)
        };
        revision
            .and_then(|revision| {
                for_board().find(|variant| variant.panel_revision == Some(revision))
            })
            // With nothing remembered, the first build listed for the board is
            // the one to try. `bundle_firmware.py` lists the current revision
            // first for exactly this reason.
            .or_else(|| for_board().next())
    }

    /// Every build that could drive this board, so a screen that comes up
    /// unreadable has somewhere to go next.
    pub fn alternatives_for(&self, board: PanelModel, written: &str) -> Vec<&FirmwareVariant> {
        self.variants
            .iter()
            .filter(|variant| variant.panel == board && variant.id != written)
            .collect()
    }
}

fn resolve_variant(
    spec: FirmwareVariantSpec,
    root: &Path,
) -> Result<FirmwareVariant, FirmwareError> {
    if spec.id.trim().is_empty() || spec.label.trim().is_empty() {
        return Err(FirmwareError::VariantIdentity);
    }
    if spec.images.is_empty() {
        return Err(FirmwareError::NoImages(spec.id));
    }

    let directory = root.join(&spec.id);
    let mut segments = Vec::with_capacity(spec.images.len());
    for image in &spec.images {
        let offset = image.parsed_offset()?;
        // Reject traversal rather than trusting a bundled manifest: it is data
        // read from disk, and a file name is never a path here.
        if image.file.contains('/') || image.file.contains('\\') || image.file.contains("..") {
            return Err(FirmwareError::ImageName {
                variant: spec.id.clone(),
                file: image.file.clone(),
            });
        }
        let path = directory.join(&image.file);
        let bytes = fs::read(&path).map_err(|error| FirmwareError::Read {
            path: path.clone(),
            detail: error.to_string(),
        })?;
        if bytes.is_empty() {
            return Err(FirmwareError::EmptyImage {
                variant: spec.id.clone(),
                file: image.file.clone(),
            });
        }
        segments.push(FlashSegment {
            offset,
            name: image.file.clone(),
            bytes,
        });
    }

    segments.sort_by_key(|segment| segment.offset);
    validate_layout(&spec.id, &segments)?;

    Ok(FirmwareVariant {
        id: spec.id,
        label: spec.label,
        panel: spec.panel,
        panel_revision: spec.panel_revision,
        segments,
    })
}

/// Rejects an image set that would produce a device that does not boot.
fn validate_layout(variant: &str, segments: &[FlashSegment]) -> Result<(), FirmwareError> {
    for offset in REQUIRED_OFFSETS {
        if !segments.iter().any(|segment| segment.offset == offset) {
            return Err(FirmwareError::MissingOffset {
                variant: variant.to_owned(),
                offset,
            });
        }
    }

    for pair in segments.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if first.offset == second.offset {
            return Err(FirmwareError::DuplicateOffset {
                variant: variant.to_owned(),
                offset: first.offset,
            });
        }
        let end = first.offset as u64 + first.bytes.len() as u64;
        if end > second.offset as u64 {
            return Err(FirmwareError::Overlap {
                variant: variant.to_owned(),
                first: first.name.clone(),
                second: second.name.clone(),
            });
        }
    }
    Ok(())
}

/// What the firmware banner told us about the attached board.
///
/// The device announces itself on boot as
/// `READY INK1 <w>x<h> <payload> <build> [panel]`. It is the same line the
/// display route reads to learn which panel it is drawing for, so the parser
/// lives beside the protocol rather than here; this module only cares about the
/// build id it carries.
///
/// Firmware predating build reporting omits that field, so a missing build is
/// "unknown", never "wrong" -- a working device must not be nagged to reflash
/// just because it cannot say which build it runs.
pub use brickellstatus_eink::DeviceBanner;

/// Why a flash is being offered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FlashReason {
    /// The board enumerated over USB but never announced itself: blank,
    /// crashed, or running unrelated firmware.
    NotResponding,
    /// The board runs a different build from the one this app ships.
    BuildMismatch { device: String, bundled: String },
    /// The board is running this firmware, but the build for another panel.
    ///
    /// Only the device can tell us this, and it does: its probe names the board
    /// it woke up on. Nothing needs to be asked, and nothing is wrong with the
    /// hardware — the right image simply has to replace the one that was
    /// written before anybody knew what this board was.
    WrongBoard { board: PanelModel },
}

/// Whether the operator should be prompted to flash.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum FlashRequirement {
    /// Nothing is plugged in.
    NoDevice,
    /// The board runs exactly what this app ships.
    UpToDate { build: String },
    /// The board works but predates build reporting, so it cannot be compared.
    /// Flashing is available, but nothing here says it is needed.
    UnknownBuild,
    /// Prompt the operator.
    Required { reason: FlashReason },
}

impl FlashRequirement {
    /// True only when the app should raise the flash prompt on its own.
    pub fn should_prompt(&self) -> bool {
        matches!(self, Self::Required { .. })
    }
}

/// What this app last wrote to a particular board.
///
/// The board's own banner is the better answer when it can be heard, but it is
/// only spoken at boot and the port is usually held by the display worker, so
/// it frequently cannot be. This is the record that does not depend on catching
/// a moment: the app knows what it flashed and to which board, and that stays
/// true across restarts, busy ports, and missed banners.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashRecord {
    /// Board identity, from its USB serial number.
    pub serial_number: String,
    /// The bundled build that was written.
    pub build: String,
    /// Which panel variant was written, so the prompt can offer the other one.
    pub variant_id: String,
    pub flashed_at: String,
}

/// What asking the board actually produced.
///
/// The distinction that matters is between a board that stayed silent and a
/// board that could not be asked. Collapsing the second into the first is what
/// made the app demand a flash on every launch: the display worker owns the
/// serial port, so the firmware probe often cannot open it, and "I could not
/// ask" was being read as "it is not running our firmware".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceProbe {
    /// No Espressif device is attached.
    NoPort,
    /// A device is attached but the port could not be opened or read — most
    /// often because something else already holds it.
    Unreachable,
    /// The port opened and the board said nothing.
    ///
    /// This is *not* evidence of a blank board. The banner is spoken once, at
    /// boot, and opening the port deliberately does not reset the board — so a
    /// board that has been running for a minute is exactly as silent as one
    /// with no firmware on it at all. Which of the two it is has to come from
    /// somewhere else: what this app remembers writing, or whether the display
    /// route is getting its frames acknowledged.
    Silent,
    /// The board spoke, and this is what it said.
    Answered(DeviceBanner),
}

/// What the live display route says about the board, which is the only
/// evidence that separates a silent working board from a silent blank one.
///
/// A board that acknowledges a frame is running this firmware; that is what
/// `ACK INK1` means. A board that refuses two frames running is not.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RouteEvidence {
    /// No display route is open to this board, so nothing is going to answer on
    /// its behalf and waiting would only postpone the question forever. This is
    /// a board nobody has asked the app to drive: the state a brand-new one is
    /// in when it is plugged in for the first time.
    #[default]
    Absent,
    /// A route is open but has not carried a frame yet. An answer is seconds
    /// away, and a prompt raised now is one that withdraws itself once the
    /// answer arrives — so nothing is claimed until it does.
    Pending,
    /// A frame was acknowledged. The board runs this firmware, whatever it did
    /// or did not say at boot.
    Acknowledged,
    /// Frames were attempted and went unacknowledged.
    Failing,
}

/// Decides whether an attached board needs the bundled firmware.
///
/// Kept free of I/O so the decision is testable without a device: every branch
/// here is reachable in a unit test, and the wrong answer either nags a working
/// device or silently leaves a dead one unflashed.
pub fn evaluate_flash_requirement(
    probe: &DeviceProbe,
    bundled_build: Option<&str>,
    remembered: Option<&FlashRecord>,
    evidence: RouteEvidence,
) -> FlashRequirement {
    let banner = match probe {
        DeviceProbe::NoPort => return FlashRequirement::NoDevice,
        // Fail toward silence. An unreachable port is not evidence about what
        // the board is running, and guessing wrong here nags someone whose
        // hardware is working perfectly.
        DeviceProbe::Unreachable => return from_record(remembered, bundled_build),
        DeviceProbe::Silent => return silent_board(remembered, bundled_build, evidence),
        DeviceProbe::Answered(banner) => banner,
    };
    if !banner.saw_ready {
        return silent_board(remembered, bundled_build, evidence);
    }
    // A board that says it is the wrong build for itself has settled the
    // question: whatever the build ids say, this image cannot drive this panel.
    if banner.mismatch
        && let Some(board) = banner.board
    {
        return FlashRequirement::Required {
            reason: FlashReason::WrongBoard { board },
        };
    }
    // The board's own word wins when it gives one; our record answers when it
    // does not. A firmware old enough to omit its build id is exactly the case
    // the record exists for.
    let reported = banner
        .build
        .as_deref()
        .or_else(|| remembered.map(|record| record.build.as_str()));
    match (reported, bundled_build) {
        (Some(device), Some(bundled)) if device == bundled => FlashRequirement::UpToDate {
            build: device.to_owned(),
        },
        (Some(device), Some(bundled)) => FlashRequirement::Required {
            reason: FlashReason::BuildMismatch {
                device: device.to_owned(),
                bundled: bundled.to_owned(),
            },
        },
        // Either side missing a build id makes comparison impossible. The
        // device is talking, so it is working; do not manufacture a mismatch.
        _ => FlashRequirement::UnknownBuild,
    }
}

/// What this app remembers writing to the board in front of it, which answers
/// for a board that cannot be asked.
fn from_record(remembered: Option<&FlashRecord>, bundled_build: Option<&str>) -> FlashRequirement {
    match (remembered, bundled_build) {
        (Some(record), Some(bundled)) if record.build == bundled => FlashRequirement::UpToDate {
            build: record.build.clone(),
        },
        _ => FlashRequirement::UnknownBuild,
    }
}

/// Decides about a board that opened its port and then said nothing.
///
/// The reported bug lives here. Silence used to mean "blank board, offer a
/// flash", but silence is the *normal* state of a healthy board: the banner is
/// spoken at boot and never again, so every launch after the first one hears
/// nothing and demanded a reflash of a board that had just been flashed. What
/// separates the two cases is never the silence itself:
///
/// - the route is delivering acknowledged frames: it is our firmware, full stop;
/// - the route is failing: it really is not answering, and that is worth a prompt;
/// - the route is open but has not answered yet: say nothing for the few seconds
///   that takes, rather than raising a demand that withdraws itself;
/// - there is no route at all: our own record of flashing this board is the
///   tiebreaker, and with no record the board is a stranger and worth offering a
///   flash to, which is how a blank board gets adopted the first time it is
///   plugged in.
fn silent_board(
    remembered: Option<&FlashRecord>,
    bundled_build: Option<&str>,
    evidence: RouteEvidence,
) -> FlashRequirement {
    match evidence {
        RouteEvidence::Acknowledged | RouteEvidence::Pending => {
            from_record(remembered, bundled_build)
        }
        RouteEvidence::Failing => FlashRequirement::Required {
            reason: FlashReason::NotResponding,
        },
        RouteEvidence::Absent if remembered.is_some() => from_record(remembered, bundled_build),
        RouteEvidence::Absent => FlashRequirement::Required {
            reason: FlashReason::NotResponding,
        },
    }
}

#[derive(Debug, Error)]
pub enum FirmwareError {
    #[error("could not read {path}: {detail}")]
    Read { path: PathBuf, detail: String },
    #[error("firmware manifest is not valid: {detail}")]
    Manifest { detail: String },
    #[error("unsupported firmware manifest schema version {0}")]
    SchemaVersion(u32),
    #[error("firmware manifest targets chip {found:?}, but this app flashes esp32s3")]
    Chip { found: String },
    #[error("firmware manifest declares no variants")]
    NoVariants,
    #[error("firmware variant {0:?} appears more than once")]
    DuplicateVariant(String),
    #[error("firmware variant requires a non-empty id and label")]
    VariantIdentity,
    #[error("firmware variant {0:?} declares no images")]
    NoImages(String),
    #[error("firmware image offset {value:?} is not a number")]
    Offset { value: String },
    #[error("firmware variant {variant:?} declares an unsafe image name {file:?}")]
    ImageName { variant: String, file: String },
    #[error("firmware image {file:?} in variant {variant:?} is empty")]
    EmptyImage { variant: String, file: String },
    #[error("firmware variant {variant:?} is missing the image at offset {offset:#x}")]
    MissingOffset { variant: String, offset: u32 },
    #[error("firmware variant {variant:?} declares two images at offset {offset:#x}")]
    DuplicateOffset { variant: String, offset: u32 },
    #[error("serial port {port}: {detail}")]
    Port { port: String, detail: String },
    #[error("flashing failed: {detail}")]
    Flash { detail: String },
    #[error("in variant {variant:?}, image {first:?} overruns {second:?}")]
    Overlap {
        variant: String,
        first: String,
        second: String,
    },
}

/// Reports flash progress to the caller.
///
/// Flashing takes tens of seconds and rewrites the bootloader, so the operator
/// needs to see it moving; a UI that looks hung invites unplugging the board
/// mid-write, which is the one action that actually bricks it.
pub trait FlashProgress: Send {
    /// A new image started writing at `offset`, `total` bytes long.
    fn segment_started(&mut self, offset: u32, total: usize);
    /// Bytes written so far within the current image.
    fn segment_advanced(&mut self, written: usize);
    /// Post-write checksum verification began.
    fn verifying(&mut self);
    /// The current image finished; `skipped` when its contents already matched.
    fn segment_finished(&mut self, skipped: bool);
}

#[cfg(desktop)]
struct ProgressAdapter<'a> {
    inner: &'a mut dyn FlashProgress,
}

#[cfg(desktop)]
impl espflash::target::ProgressCallbacks for ProgressAdapter<'_> {
    fn init(&mut self, addr: u32, total: usize) {
        self.inner.segment_started(addr, total);
    }

    fn update(&mut self, current: usize) {
        self.inner.segment_advanced(current);
    }

    fn verifying(&mut self) {
        self.inner.verifying();
    }

    fn finish(&mut self, skipped: bool) {
        self.inner.segment_finished(skipped);
    }
}

/// Writes a validated variant to the board on `port_name`.
///
/// Blocking: espflash drives the serial bootloader synchronously, so callers on
/// an async runtime must move this onto a blocking thread.
///
/// Desktop only: flashing means opening a USB serial bootloader, which an
/// unprivileged Android app cannot do. The mobile build ships no firmware
/// either, so `flash_firmware` refuses before it ever reaches this.
#[cfg(desktop)]
pub fn flash_variant(
    port_name: &str,
    variant: &FirmwareVariant,
    progress: &mut dyn FlashProgress,
) -> Result<(), FirmwareError> {
    use espflash::{
        connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
        flasher::Flasher,
        image_format::Segment,
        target::Chip,
    };

    let usb_info = serialport::available_ports()
        .map_err(|error| FirmwareError::Port {
            port: port_name.to_owned(),
            detail: error.to_string(),
        })?
        .into_iter()
        .find(|port| port.port_name == port_name)
        .and_then(|port| match port.port_type {
            serialport::SerialPortType::UsbPort(info) => Some(info),
            _ => None,
        })
        .ok_or_else(|| FirmwareError::Port {
            port: port_name.to_owned(),
            detail: "no USB serial device is present at this port".into(),
        })?;

    let serial = serialport::new(port_name, 115_200)
        .flow_control(serialport::FlowControl::None)
        .open_native()
        .map_err(|error| FirmwareError::Port {
            port: port_name.to_owned(),
            detail: error.to_string(),
        })?;

    let connection = Connection::new(
        serial,
        usb_info,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );

    // verify=true makes the flasher read back a checksum of everything it
    // wrote. This runs once, on hardware the user is holding, so paying for
    // verification beats shipping a board that silently took a bad write.
    let mut flasher = Flasher::connect(
        connection,
        true,
        true,
        false,
        Some(Chip::Esp32s3),
        Some(921_600),
    )
    .map_err(|error| FirmwareError::Flash {
        detail: error.to_string(),
    })?;

    let segments = variant
        .segments
        .iter()
        .map(|segment| Segment::new(segment.offset, &segment.bytes))
        .collect::<Vec<_>>();

    let mut adapter = ProgressAdapter { inner: progress };
    flasher
        .write_bins_to_flash(&segments, &mut adapter)
        .map_err(|error| FirmwareError::Flash {
            detail: error.to_string(),
        })
}

/// Mobile counterpart of [`flash_variant`], kept so `flash_firmware` compiles
/// unchanged on both platforms.
///
/// Unreachable in practice: the Android bundle ships no firmware resource, so
/// the command refuses at `firmware_root` long before this. It exists to keep
/// the one call site free of platform branching.
#[cfg(mobile)]
pub fn flash_variant(
    port_name: &str,
    _variant: &FirmwareVariant,
    _progress: &mut dyn FlashProgress,
) -> Result<(), FirmwareError> {
    Err(FirmwareError::Port {
        port: port_name.to_owned(),
        detail: "flashing a board over USB is not available on this device".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("brickellstatus-firmware-{name}"));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn image(&self, variant: &str, file: &str, len: usize) {
            let directory = self.root.join(variant);
            fs::create_dir_all(&directory).unwrap();
            let mut handle = fs::File::create(directory.join(file)).unwrap();
            handle.write_all(&vec![0xa5; len]).unwrap();
        }

        fn manifest(&self, json: &str) {
            fs::write(self.root.join("manifest.json"), json).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn manifest_json(images: &str) -> String {
        format!(
            r#"{{
              "schemaVersion": 2,
              "chip": "esp32s3",
              "sourceRevision": "abc1234",
              "variants": [
                {{
                  "id": "vision-master-e213",
                  "label": "Vision Master E213 (original panel)",
                  "panel": "e213",
                  "panelRevision": "original",
                  "images": [{images}]
                }}
              ]
            }}"#
        )
    }

    const FULL_IMAGES: &str = r#"
        {"offset": "0x0", "file": "bootloader.bin"},
        {"offset": "0x8000", "file": "partitions.bin"},
        {"offset": "0xe000", "file": "boot_app0.bin"},
        {"offset": "0x10000", "file": "firmware.bin"}
    "#;

    fn write_full_images(fixture: &Fixture) {
        write_variant_images(fixture, "vision-master-e213");
    }

    fn write_variant_images(fixture: &Fixture, variant: &str) {
        fixture.image(variant, "bootloader.bin", 15_104);
        fixture.image(variant, "partitions.bin", 3_072);
        fixture.image(variant, "boot_app0.bin", 8_192);
        fixture.image(variant, "firmware.bin", 502_736);
    }

    /// The shipped shape: one build per board, and two for the board whose
    /// panel revision nothing can read back.
    fn every_board_manifest() -> String {
        format!(
            r#"{{
              "schemaVersion": 2,
              "chip": "esp32s3",
              "sourceRevision": "abc1234",
              "variants": [
                {{"id":"vision-master-e213","label":"Original panel","panel":"e213","panelRevision":"original","images":[{FULL_IMAGES}]}},
                {{"id":"vision-master-e213-v11","label":"Panel v1.1","panel":"e213","panelRevision":"v11","images":[{FULL_IMAGES}]}},
                {{"id":"vision-master-e290","label":"E290","panel":"e290","images":[{FULL_IMAGES}]}}
              ]
            }}"#
        )
    }

    #[test]
    fn loads_a_complete_bundle() {
        let fixture = Fixture::new("complete");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(FULL_IMAGES));

        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        assert_eq!(bundle.source_revision.as_deref(), Some("abc1234"));
        let variant = bundle.variant("vision-master-e213").unwrap();
        assert_eq!(variant.panel_revision, Some(PanelRevision::Original));
        assert_eq!(variant.segments.len(), 4);
        assert_eq!(variant.total_bytes(), 15_104 + 3_072 + 8_192 + 502_736);
        // Sorted by offset so the flasher writes the bootloader first.
        assert_eq!(
            variant
                .segments
                .iter()
                .map(|segment| segment.offset)
                .collect::<Vec<_>>(),
            vec![0x0, 0x8000, 0xe000, 0x10000]
        );
    }

    #[test]
    fn rejects_a_bundle_missing_the_ota_data_image() {
        // The shipped partition table has app0/app1 slots. Without boot_app0 at
        // 0xe000 a board previously booting app1 keeps booting app1, which this
        // flash never wrote -- it comes back running the old firmware, or none.
        let fixture = Fixture::new("no-otadata");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(
            r#"
            {"offset": "0x0", "file": "bootloader.bin"},
            {"offset": "0x8000", "file": "partitions.bin"},
            {"offset": "0x10000", "file": "firmware.bin"}
        "#,
        ));
        let error = FirmwareBundle::load(&fixture.root).unwrap_err();
        assert!(
            matches!(error, FirmwareError::MissingOffset { offset, .. } if offset == OTA_DATA_OFFSET),
            "got {error}"
        );
    }

    #[test]
    fn rejects_a_bundle_missing_the_bootloader() {
        let fixture = Fixture::new("no-bootloader");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(
            r#"
            {"offset": "0x8000", "file": "partitions.bin"},
            {"offset": "0xe000", "file": "boot_app0.bin"},
            {"offset": "0x10000", "file": "firmware.bin"}
        "#,
        ));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::MissingOffset { offset, .. } if offset == BOOTLOADER_OFFSET
        ));
    }

    #[test]
    fn rejects_overlapping_images() {
        // An oversized partition table would run into otadata and produce a
        // device that enumerates but never boots.
        let fixture = Fixture::new("overlap");
        fixture.image("vision-master-e213", "bootloader.bin", 15_104);
        fixture.image("vision-master-e213", "partitions.bin", 0x7_000);
        fixture.image("vision-master-e213", "boot_app0.bin", 8_192);
        fixture.image("vision-master-e213", "firmware.bin", 1_024);
        fixture.manifest(&manifest_json(FULL_IMAGES));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::Overlap { .. }
        ));
    }

    #[test]
    fn rejects_an_empty_image() {
        let fixture = Fixture::new("empty");
        write_full_images(&fixture);
        fixture.image("vision-master-e213", "firmware.bin", 0);
        fixture.manifest(&manifest_json(FULL_IMAGES));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::EmptyImage { .. }
        ));
    }

    #[test]
    fn rejects_a_manifest_for_another_chip() {
        let fixture = Fixture::new("wrong-chip");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(FULL_IMAGES).replace("esp32s3", "esp32c3"));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::Chip { .. }
        ));
    }

    #[test]
    fn rejects_an_image_name_that_escapes_the_variant_directory() {
        let fixture = Fixture::new("traversal");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(
            r#"
            {"offset": "0x0", "file": "../../etc/passwd"},
            {"offset": "0x8000", "file": "partitions.bin"},
            {"offset": "0xe000", "file": "boot_app0.bin"},
            {"offset": "0x10000", "file": "firmware.bin"}
        "#,
        ));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::ImageName { .. }
        ));
    }

    #[test]
    fn rejects_an_unparseable_offset() {
        let fixture = Fixture::new("bad-offset");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(
            r#"
            {"offset": "start", "file": "bootloader.bin"},
            {"offset": "0x8000", "file": "partitions.bin"},
            {"offset": "0xe000", "file": "boot_app0.bin"},
            {"offset": "0x10000", "file": "firmware.bin"}
        "#,
        ));
        assert!(matches!(
            FirmwareBundle::load(&fixture.root).unwrap_err(),
            FirmwareError::Offset { .. }
        ));
    }

    #[test]
    fn selects_a_build_by_panel_revision() {
        let fixture = Fixture::new("two-variants");
        for variant in [
            "vision-master-e213",
            "vision-master-e213-v11",
            "vision-master-e290",
        ] {
            write_variant_images(&fixture, variant);
        }
        fixture.manifest(
            &r#"{
              "schemaVersion": 2,
              "chip": "esp32s3",
              "variants": [
                {"id":"vision-master-e213","label":"Original panel","panel":"e213","panelRevision":"original","images":[IMAGES]},
                {"id":"vision-master-e213-v11","label":"Panel v1.1","panel":"e213","panelRevision":"v11","images":[IMAGES]},
                {"id":"vision-master-e290","label":"E290","panel":"e290","images":[IMAGES]}
              ]
            }"#
            .replace("IMAGES", FULL_IMAGES),
        );

        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        assert_eq!(bundle.variants().len(), 3);
        // The remembered revision picks between the two E213 builds...
        assert_eq!(
            bundle
                .for_board(PanelModel::E213, Some(PanelRevision::V11))
                .unwrap()
                .id,
            "vision-master-e213-v11"
        );
        assert_eq!(
            bundle
                .for_board(PanelModel::E213, Some(PanelRevision::Original))
                .unwrap()
                .id,
            "vision-master-e213"
        );
        // ...and means nothing on a board with only one build, which must be
        // selected by the board alone rather than falling through to a
        // revision that does not apply to it.
        assert_eq!(
            bundle
                .for_board(PanelModel::E290, Some(PanelRevision::V11))
                .unwrap()
                .id,
            "vision-master-e290"
        );
        assert_eq!(
            bundle.for_board(PanelModel::E290, None).unwrap().id,
            "vision-master-e290"
        );
    }

    /// Nothing remembered, and a board that has never spoken: the first build
    /// listed for it is written, and the firmware reports back if that was the
    /// wrong guess.
    #[test]
    fn a_board_with_nothing_remembered_still_has_a_build_to_write() {
        let fixture = Fixture::new("unremembered");
        write_variant_images(&fixture, "vision-master-e213");
        write_variant_images(&fixture, "vision-master-e213-v11");
        write_variant_images(&fixture, "vision-master-e290");
        fixture.manifest(&every_board_manifest());
        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        assert!(bundle.for_board(PanelModel::E213, None).is_some());
        assert_eq!(
            bundle
                .alternatives_for(PanelModel::E213, "vision-master-e213")
                .iter()
                .map(|variant| variant.id.as_str())
                .collect::<Vec<_>>(),
            vec!["vision-master-e213-v11"],
            "the other E213 build is what an unreadable screen is offered next"
        );
        assert!(
            bundle
                .alternatives_for(PanelModel::E290, "vision-master-e290")
                .is_empty(),
            "a board with one build has nothing else to try"
        );
    }

    #[test]
    fn reports_a_missing_bundle_rather_than_panicking() {
        let missing = std::env::temp_dir().join("brickellstatus-firmware-absent");
        let _ = fs::remove_dir_all(&missing);
        assert!(matches!(
            FirmwareBundle::load(&missing).unwrap_err(),
            FirmwareError::Read { .. }
        ));
    }

    #[test]
    fn parses_a_banner_with_a_build_id() {
        let banner = DeviceBanner::parse("READY INK1 250x122 3904 abc1234\n");
        assert!(banner.saw_ready);
        assert_eq!(banner.build.as_deref(), Some("abc1234"));
    }

    #[test]
    fn parses_an_older_banner_without_a_build_id() {
        let banner = DeviceBanner::parse("READY INK1 250x122 3904");
        assert!(banner.saw_ready);
        assert_eq!(banner.build, None);
    }

    #[test]
    fn ignores_unrelated_serial_chatter() {
        for line in [
            "",
            "NACK CRC",
            "ets Jun  8 2016 00:22:57",
            "rst:0x1 (POWERON)",
        ] {
            let banner = DeviceBanner::parse(line);
            assert!(!banner.saw_ready, "{line:?} must not read as a banner");
        }
    }

    /// The manifest and the images have to agree about what build this is, and
    /// nothing in either file forces them to: they are produced by two scripts
    /// that derive the same identity separately. When they disagreed, every
    /// board reported a build the app had never heard of, the app offered an
    /// upgrade, and the upgrade produced a board that disagreed exactly as much
    /// as before — a reflash that could never succeed, on every launch.
    ///
    /// Reads the bytes rather than the device: the id is a string literal in the
    /// image, so the two can be compared here, on any machine, with no board
    /// attached and no toolchain installed.
    #[test]
    fn the_shipped_images_announce_the_build_the_manifest_claims() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/firmware");
        if !root.join("manifest.json").is_file() {
            return;
        }
        let Ok(bundle) = FirmwareBundle::load(&root) else {
            return;
        };
        let Some(revision) = bundle.source_revision.as_deref() else {
            return;
        };
        assert_ne!(
            revision,
            brickellstatus_eink::UNKNOWN_BUILD,
            "a bundle that cannot name its own build must not be shipped"
        );
        for variant in bundle.variants() {
            let application = variant
                .segments
                .iter()
                .find(|segment| segment.offset == APP_OFFSET)
                .expect("a validated variant always has an application image");
            assert!(
                application
                    .bytes
                    .windows(revision.len())
                    .any(|window| window == revision.as_bytes()),
                "{} ships an image that never says {revision:?}, so every board \
                 flashed from it reports a build this app will not recognise",
                variant.id
            );
        }
    }

    #[test]
    fn no_device_means_no_prompt() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::NoPort,
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert_eq!(requirement, FlashRequirement::NoDevice);
        assert!(!requirement.should_prompt());
    }

    #[test]
    fn an_unknown_silent_board_is_offered_a_flash() {
        // Nothing has been written to this board by us and nothing has been
        // delivered to it. Adopting a blank board on first run depends on this.
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(requirement.should_prompt());
        assert_eq!(
            requirement,
            FlashRequirement::Required {
                reason: FlashReason::NotResponding
            }
        );
    }

    #[test]
    fn a_matching_build_is_left_alone() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 abc1234")),
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(
            requirement,
            FlashRequirement::UpToDate {
                build: "abc1234".into()
            }
        );
    }

    #[test]
    fn a_different_build_is_prompted_to_flash() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 old0001")),
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(requirement.should_prompt());
        assert_eq!(
            requirement,
            FlashRequirement::Required {
                reason: FlashReason::BuildMismatch {
                    device: "old0001".into(),
                    bundled: "abc1234".into()
                }
            }
        );
    }

    #[test]
    fn a_working_board_that_cannot_report_its_build_is_not_nagged() {
        // Firmware predating build reporting still works. Treating "cannot say"
        // as "wrong" would prompt a reflash every launch for no reason.
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904")),
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// A firmware built outside a git checkout stamps "unknown" rather than
    /// inventing an identity. Comparing that against a real revision reported
    /// every such board as outdated, and the reflash it offered produced
    /// another board saying "unknown" -- an upgrade that could never complete.
    #[test]
    fn a_board_that_cannot_name_its_build_is_not_a_mismatch() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 unknown")),
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(
            !requirement.should_prompt(),
            "an unnamed build is an absent build id, never a differing one"
        );
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    #[test]
    fn a_bundle_without_a_revision_cannot_declare_a_mismatch() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 abc1234")),
            None,
            None,
            RouteEvidence::Absent,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// The bug this exists to prevent: the display worker owns the serial port,
    /// so the firmware probe frequently cannot open it. Reading that as "the
    /// board did not answer" demanded a flash on every single launch of a
    /// perfectly working device.
    #[test]
    fn a_port_that_cannot_be_opened_is_never_read_as_an_unflashed_board() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Unreachable,
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert!(
            !requirement.should_prompt(),
            "an unreachable port must not raise the flash prompt"
        );
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    fn record(build: &str) -> FlashRecord {
        FlashRecord {
            serial_number: "F0:9E:9E:3B:26:B4".into(),
            build: build.into(),
            variant_id: "vision-master-e213".into(),
            flashed_at: "2026-08-16T13:00:00Z".into(),
        }
    }

    /// The reported case: flashed repeatedly, prompted again on every launch.
    /// The banner naming the build is only spoken at boot and the display
    /// worker holds the port, so the board that is running exactly this build
    /// usually cannot say so. What this app wrote is the answer that survives.
    #[test]
    fn a_board_we_flashed_ourselves_is_up_to_date_even_when_it_cannot_be_reached() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Unreachable,
            Some("abc1234"),
            Some(&record("abc1234")),
            RouteEvidence::Absent,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(
            requirement,
            FlashRequirement::UpToDate {
                build: "abc1234".into()
            }
        );
    }

    /// ...but the record only speaks for the build it actually wrote. A newer
    /// app shipping a newer build must still offer to flash it.
    #[test]
    fn a_stale_record_does_not_suppress_a_real_upgrade() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Unreachable,
            Some("def5678"),
            Some(&record("abc1234")),
            RouteEvidence::Absent,
        );
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// The board's own word outranks our memory of it. Someone flashing the
    /// board from esptool behind our back is exactly why the record cannot be
    /// the only source.
    #[test]
    fn the_boards_own_banner_wins_over_what_we_remember_writing() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 other999")),
            Some("abc1234"),
            Some(&record("abc1234")),
            RouteEvidence::Absent,
        );
        assert_eq!(
            requirement,
            FlashRequirement::Required {
                reason: FlashReason::BuildMismatch {
                    device: "other999".into(),
                    bundled: "abc1234".into()
                }
            }
        );
    }

    /// A firmware old enough to omit its build id still answers; the record
    /// fills in what the banner left out.
    #[test]
    fn a_record_names_the_build_a_silent_banner_omits() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904")),
            Some("abc1234"),
            Some(&record("abc1234")),
            RouteEvidence::Absent,
        );
        assert_eq!(
            requirement,
            FlashRequirement::UpToDate {
                build: "abc1234".into()
            }
        );
    }

    /// The reported bug, exactly: flash the board, quit, relaunch, and be asked
    /// to flash the board that was just flashed.
    ///
    /// The banner is spoken once at boot. The app connects to a board that has
    /// been up for a while, hears nothing, and used to read that silence as an
    /// unflashed board -- so the answer to "did this work?" was always "do it
    /// again". A board this app wrote is not silent-because-blank.
    #[test]
    fn a_board_we_just_flashed_is_not_asked_to_flash_again() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            Some(&record("abc1234")),
            RouteEvidence::Absent,
        );
        assert!(
            !requirement.should_prompt(),
            "a board this app flashed must not be offered the same flash again"
        );
        assert_eq!(
            requirement,
            FlashRequirement::UpToDate {
                build: "abc1234".into()
            }
        );
    }

    /// Silence from a board that is acknowledging frames says nothing at all:
    /// ACK INK1 is proof this firmware is running, whatever the board said or
    /// did not say at boot. This is the case with no record to fall back on --
    /// a board someone else flashed, which must also not be nagged.
    /// A build that landed on the other board says so, and that outranks every
    /// other signal: the build ids can agree while the image is still unable to
    /// drive the panel in front of it.
    #[test]
    fn a_build_on_the_wrong_board_is_always_reflashed() {
        let banner = DeviceBanner::parse("READY INK1 0x0 0 abc1234 E290 MISMATCH");
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(banner),
            Some("abc1234"),
            None,
            RouteEvidence::Absent,
        );
        assert_eq!(
            requirement,
            FlashRequirement::Required {
                reason: FlashReason::WrongBoard {
                    board: PanelModel::E290
                }
            }
        );
        assert!(requirement.should_prompt());
    }

    /// The board it landed on is what the app writes next, without asking.
    #[test]
    fn the_reported_board_selects_the_build_that_replaces_it() {
        let fixture = Fixture::new("wrong-board");
        for variant in [
            "vision-master-e213",
            "vision-master-e213-v11",
            "vision-master-e290",
        ] {
            write_variant_images(&fixture, variant);
        }
        fixture.manifest(&every_board_manifest());
        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        let banner = DeviceBanner::parse("READY INK1 0x0 0 abc1234 E290 MISMATCH");
        let board = banner.board.expect("the firmware named the board");
        assert_eq!(
            bundle.for_board(board, None).unwrap().id,
            "vision-master-e290"
        );
    }

    /// A board running the right build for itself is left alone, panel name and
    /// all.
    #[test]
    fn a_board_on_its_own_build_is_up_to_date() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 296x128 4736 abc1234 E290")),
            Some("abc1234"),
            None,
            RouteEvidence::Acknowledged,
        );
        assert_eq!(
            requirement,
            FlashRequirement::UpToDate {
                build: "abc1234".into()
            }
        );
    }

    #[test]
    fn a_board_that_acknowledges_frames_is_never_called_unresponsive() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            None,
            RouteEvidence::Acknowledged,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// ...and the converse, which is what keeps the prompt reachable: a board
    /// that will not take a frame is genuinely not answering, and is offered a
    /// flash even though this app is the one that flashed it. Without this, a
    /// board that dies after we wrote it could never be recovered from the app.
    #[test]
    fn a_board_that_refuses_frames_is_offered_a_flash_despite_our_record() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            Some(&record("abc1234")),
            RouteEvidence::Failing,
        );
        assert!(requirement.should_prompt());
        assert_eq!(
            requirement,
            FlashRequirement::Required {
                reason: FlashReason::NotResponding
            }
        );
    }

    /// An open route that has not carried its first frame yet answers within
    /// seconds. Prompting into that window put a demand on screen that took
    /// itself back once the frame landed, on every launch of a working desk.
    #[test]
    fn a_route_that_has_not_answered_yet_is_given_the_chance_to() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            None,
            RouteEvidence::Pending,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// ...and it is only a grace period. A board that never answers still ends
    /// up offered a flash, so nothing here can quietly swallow the prompt.
    #[test]
    fn the_grace_period_ends_when_the_route_gives_up() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Silent,
            Some("abc1234"),
            None,
            RouteEvidence::Failing,
        );
        assert!(requirement.should_prompt());
    }

    /// A board that answers with a stale build is still an upgrade offer even
    /// while it is happily delivering frames: working is not the same as current.
    #[test]
    fn a_delivering_board_running_an_old_build_is_still_offered_the_new_one() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 old0001")),
            Some("abc1234"),
            None,
            RouteEvidence::Acknowledged,
        );
        assert!(requirement.should_prompt());
    }
}
