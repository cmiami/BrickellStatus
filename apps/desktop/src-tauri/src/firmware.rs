//! Bundled E213 firmware and the USB flash workflow.
//!
//! The app ships the firmware it expects the device to run, so a user who plugs
//! in a board can bring it up without a toolchain. Detection of the board is
//! automatic; the *panel revision* is not, and cannot be.
//!
//! `platformio.ini` builds two environments whose only difference is a
//! compile-time driver class:
//!
//! ```text
//! #if TENDERS_LOG_PANEL_V11
//! EInkDisplay_VisionMasterE213V1_1 display;
//! #else
//! EInkDisplay_VisionMasterE213 display;
//! #endif
//! ```
//!
//! Same ESP32-S3, same USB VID/PID; the difference is the physical panel. So
//! the revision is a question for the operator, remembered once, with a
//! re-flash of the other variant as the recovery when the screen comes up
//! garbled.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

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
/// being flashed onto an E213.
pub const EXPECTED_CHIP: &str = "esp32s3";

/// Which physical e-paper panel a build drives.
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
    pub panel_revision: PanelRevision,
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
    pub panel_revision: PanelRevision,
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
        if manifest.schema_version != 1 {
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

    /// The build matching a panel revision, which is how the operator's stored
    /// answer maps onto an image set.
    pub fn for_panel(&self, panel: PanelRevision) -> Option<&FirmwareVariant> {
        self.variants
            .iter()
            .find(|variant| variant.panel_revision == panel)
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
/// The device announces itself on boot as `READY INK1 <w>x<h> <payload> <build>`.
/// Firmware predating build reporting omits the last field, so a missing build
/// is "unknown", never "wrong" -- a working device must not be nagged to
/// reflash just because it cannot say which build it runs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceBanner {
    /// Whether `READY INK1` was seen at all.
    pub saw_ready: bool,
    /// Build identity, when the firmware is new enough to report one.
    pub build: Option<String>,
}

impl DeviceBanner {
    /// Reads a banner line emitted by the device.
    pub fn parse(line: &str) -> Self {
        let trimmed = line.trim();
        if !(trimmed.contains("READY INK1") || trimmed == "READY") {
            return Self::default();
        }
        // `READY INK1 250x122 3904 <build>`; anything before the build id is
        // geometry the host already knows, so only the 5th token is read.
        let build = trimmed
            .split_whitespace()
            .nth(4)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Self {
            saw_ready: true,
            build,
        }
    }
}

/// Why a flash is being offered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FlashReason {
    /// The board enumerated over USB but never announced itself: blank,
    /// crashed, or running unrelated firmware.
    NotResponding,
    /// The board runs a different build from the one this app ships.
    BuildMismatch { device: String, bundled: String },
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
    /// The port was opened and the board was given its chance to speak.
    Answered(DeviceBanner),
}

/// Decides whether an attached board needs the bundled firmware.
///
/// Kept free of I/O so the decision is testable without a device: every branch
/// here is reachable in a unit test, and the wrong answer either nags a working
/// device or silently leaves a dead one unflashed.
pub fn evaluate_flash_requirement(
    probe: &DeviceProbe,
    bundled_build: Option<&str>,
) -> FlashRequirement {
    let banner = match probe {
        DeviceProbe::NoPort => return FlashRequirement::NoDevice,
        // Fail toward silence. An unreachable port is not evidence about what
        // the board is running, and guessing wrong here nags someone whose
        // hardware is working perfectly.
        DeviceProbe::Unreachable => return FlashRequirement::UnknownBuild,
        DeviceProbe::Answered(banner) => banner,
    };
    if !banner.saw_ready {
        return FlashRequirement::Required {
            reason: FlashReason::NotResponding,
        };
    }
    match (banner.build.as_deref(), bundled_build) {
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

struct ProgressAdapter<'a> {
    inner: &'a mut dyn FlashProgress,
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("tenders-firmware-{name}"));
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
              "schemaVersion": 1,
              "chip": "esp32s3",
              "sourceRevision": "abc1234",
              "variants": [
                {{
                  "id": "vision-master-e213",
                  "label": "Vision Master E213 (original panel)",
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
        fixture.image("vision-master-e213", "bootloader.bin", 15_104);
        fixture.image("vision-master-e213", "partitions.bin", 3_072);
        fixture.image("vision-master-e213", "boot_app0.bin", 8_192);
        fixture.image("vision-master-e213", "firmware.bin", 502_736);
    }

    #[test]
    fn loads_a_complete_bundle() {
        let fixture = Fixture::new("complete");
        write_full_images(&fixture);
        fixture.manifest(&manifest_json(FULL_IMAGES));

        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        assert_eq!(bundle.source_revision.as_deref(), Some("abc1234"));
        let variant = bundle.variant("vision-master-e213").unwrap();
        assert_eq!(variant.panel_revision, PanelRevision::Original);
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
        for variant in ["vision-master-e213", "vision-master-e213-v11"] {
            fixture.image(variant, "bootloader.bin", 15_104);
            fixture.image(variant, "partitions.bin", 3_072);
            fixture.image(variant, "boot_app0.bin", 8_192);
            fixture.image(variant, "firmware.bin", 501_888);
        }
        fixture.manifest(
            &r#"{
              "schemaVersion": 1,
              "chip": "esp32s3",
              "variants": [
                {"id":"vision-master-e213","label":"Original panel","panelRevision":"original","images":[IMAGES]},
                {"id":"vision-master-e213-v11","label":"Panel v1.1","panelRevision":"v11","images":[IMAGES]}
              ]
            }"#
            .replace("IMAGES", FULL_IMAGES),
        );

        let bundle = FirmwareBundle::load(&fixture.root).unwrap();
        assert_eq!(bundle.variants().len(), 2);
        assert_eq!(
            bundle.for_panel(PanelRevision::V11).unwrap().id,
            "vision-master-e213-v11"
        );
        assert_eq!(
            bundle.for_panel(PanelRevision::Original).unwrap().id,
            "vision-master-e213"
        );
    }

    #[test]
    fn reports_a_missing_bundle_rather_than_panicking() {
        let missing = std::env::temp_dir().join("tenders-firmware-absent");
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

    #[test]
    fn no_device_means_no_prompt() {
        let requirement = evaluate_flash_requirement(&DeviceProbe::NoPort, Some("abc1234"));
        assert_eq!(requirement, FlashRequirement::NoDevice);
        assert!(!requirement.should_prompt());
    }

    #[test]
    fn a_silent_board_is_prompted_to_flash() {
        // Enumerated over USB but never announced itself: blank or crashed.
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner {
                saw_ready: false,
                build: None,
            }),
            Some("abc1234"),
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
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    #[test]
    fn a_bundle_without_a_revision_cannot_declare_a_mismatch() {
        let requirement = evaluate_flash_requirement(
            &DeviceProbe::Answered(DeviceBanner::parse("READY INK1 250x122 3904 abc1234")),
            None,
        );
        assert!(!requirement.should_prompt());
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    #[test]
    fn the_bundle_this_build_ships_is_valid() {
        // Guards the bundling script, not the loader: a manifest that names a
        // file the script forgot to copy, or omits boot_app0, would otherwise
        // only surface on a device. Skips when no bundle has been generated, so
        // a checkout without an embedded toolchain still runs the suite.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/firmware");
        if !root.join("manifest.json").is_file() {
            return;
        }
        match FirmwareBundle::load(&root) {
            Ok(bundle) => {
                for variant in bundle.variants() {
                    assert!(
                        variant.total_bytes() > 100_000,
                        "{} looks too small to be a real build",
                        variant.id
                    );
                }
            }
            // An empty bundle is legitimate: the app then reports that it ships
            // no firmware. Any other failure is a broken bundling script.
            Err(FirmwareError::NoVariants) => {}
            Err(error) => panic!("bundled firmware is invalid: {error}"),
        }
    }

    /// The bug this exists to prevent: the display worker owns the serial port,
    /// so the firmware probe frequently cannot open it. Reading that as "the
    /// board did not answer" demanded a flash on every single launch of a
    /// perfectly working device.
    #[test]
    fn a_port_that_cannot_be_opened_is_never_read_as_an_unflashed_board() {
        let requirement = evaluate_flash_requirement(&DeviceProbe::Unreachable, Some("abc1234"));
        assert!(
            !requirement.should_prompt(),
            "an unreachable port must not raise the flash prompt"
        );
        assert_eq!(requirement, FlashRequirement::UnknownBuild);
    }

    /// ...while a board that genuinely stays silent still gets prompted, so the
    /// fix above cannot be mistaken for suppressing the prompt entirely.
    #[test]
    fn silence_from_a_reachable_board_still_prompts() {
        assert!(
            evaluate_flash_requirement(
                &DeviceProbe::Answered(DeviceBanner {
                    saw_ready: false,
                    build: None
                }),
                Some("abc1234")
            )
            .should_prompt()
        );
    }
}
