//! The rendering core, exposed to a JavaScript host.
//!
//! A Cloudflare Worker calls this to turn a snapshot into the exact bytes a
//! panel expects. It is deliberately thin: everything it does is already
//! implemented in `bridgestatus-projection` and `bridgestatus-eink`, and this
//! crate only crosses the boundary.
//!
//! That thinness is the point. The hosted service and the desktop app run the
//! same projection and the same renderer, so a frame produced here is
//! byte-identical to one produced locally — which is asserted by test, not
//! assumed.

use bridgestatus_contract::AppSnapshot;
use bridgestatus_eink::{RefreshMode, encode_packet};
use bridgestatus_projection::render_live_bridge_frame;
use wasm_bindgen::prelude::*;

/// Renders a bridge snapshot to the packed 3,904-byte framebuffer.
///
/// Takes and returns the plainest possible types so the host needs no
/// knowledge of the domain: JSON in, bytes out.
#[wasm_bindgen]
pub fn render_bridge_frame(snapshot_json: &str) -> Result<Vec<u8>, JsError> {
    frame_bytes(snapshot_json).map_err(|error| JsError::new(&error))
}

/// The same work without the JavaScript boundary, so the behaviour can be
/// tested on the host rather than only in a browser.
pub fn frame_bytes(snapshot_json: &str) -> Result<Vec<u8>, String> {
    let snapshot = snapshot_from_json(snapshot_json)?;
    let frame =
        render_live_bridge_frame(&snapshot).map_err(|error| format!("render failed: {error}"))?;
    Ok(frame.packed().to_vec())
}

/// Renders a bridge snapshot to a complete INK1 packet, header and CRC
/// included — what a client writes straight to the panel without understanding
/// any of it.
#[wasm_bindgen]
pub fn render_bridge_packet(snapshot_json: &str, full_refresh: bool) -> Result<Vec<u8>, JsError> {
    packet_bytes(snapshot_json, full_refresh).map_err(|error| JsError::new(&error))
}

/// Host-testable counterpart to [`render_bridge_packet`].
pub fn packet_bytes(snapshot_json: &str, full_refresh: bool) -> Result<Vec<u8>, String> {
    let snapshot = snapshot_from_json(snapshot_json)?;
    let frame =
        render_live_bridge_frame(&snapshot).map_err(|error| format!("render failed: {error}"))?;
    let refresh = if full_refresh {
        RefreshMode::Full
    } else {
        RefreshMode::Fast
    };
    Ok(encode_packet(&frame, refresh))
}

/// The panel's dimensions, so a host never hard-codes them.
#[wasm_bindgen]
pub fn panel_geometry() -> Vec<u32> {
    vec![
        u32::from(bridgestatus_eink::WIDTH),
        u32::from(bridgestatus_eink::HEIGHT),
        bridgestatus_eink::PAYLOAD_SIZE as u32,
    ]
}

fn snapshot_from_json(json: &str) -> Result<AppSnapshot, String> {
    serde_json::from_str(json).map_err(|error| format!("snapshot is not valid: {error}"))
}

#[cfg(test)]
mod tests {
    use bridgestatus_projection::render_live_bridge_frame;
    use bridgestatus_runtime::{RuntimeConfig, RuntimeEngine};
    use sha2::{Digest, Sha256};
    use tenders_storage::Store;

    use super::*;

    async fn live_snapshot() -> AppSnapshot {
        let store = Store::in_memory().await.unwrap();
        RuntimeEngine::new(store, RuntimeConfig::default())
            .await
            .unwrap()
            .get_snapshot()
            .await
            .unwrap()
    }

    /// The claim the whole split rests on: a frame rendered in a Worker is the
    /// frame the desktop app would have drawn.
    ///
    /// The risk is not the renderer — both call the same one. It is the journey
    /// in between. A Worker receives a snapshot as JSON, so anything serde
    /// rounds off, reorders, or defaults on the way through would produce a
    /// plausible frame that is quietly not the same one, and nothing would fail.
    #[tokio::test]
    async fn a_snapshot_survives_the_json_boundary_byte_for_byte() {
        let snapshot = live_snapshot().await;
        let direct = render_live_bridge_frame(&snapshot).unwrap();
        let through_json = frame_bytes(&serde_json::to_string(&snapshot).unwrap()).unwrap();

        assert_eq!(
            direct.packed().as_slice(),
            through_json.as_slice(),
            "the hosted render diverged from the local one"
        );
        assert_eq!(through_json.len(), bridgestatus_eink::PAYLOAD_SIZE);
    }

    /// The packet a client writes to the panel, header and CRC included, is
    /// likewise unchanged by the crossing.
    #[tokio::test]
    async fn the_packet_is_identical_across_the_boundary() {
        let snapshot = live_snapshot().await;
        let json = serde_json::to_string(&snapshot).unwrap();
        for full_refresh in [true, false] {
            let local = bridgestatus_eink::encode_packet(
                &render_live_bridge_frame(&snapshot).unwrap(),
                if full_refresh {
                    RefreshMode::Full
                } else {
                    RefreshMode::Fast
                },
            );
            let hosted = packet_bytes(&json, full_refresh).unwrap();
            assert_eq!(local, hosted, "full_refresh = {full_refresh}");
            assert_eq!(hosted.len(), bridgestatus_eink::PACKET_SIZE);
        }
    }

    /// Rendering the same snapshot twice must not depend on anything ambient.
    /// A clock or an iteration order leaking in here would show up as a panel
    /// that repaints for no reason.
    #[tokio::test]
    async fn the_same_snapshot_always_produces_the_same_bytes() {
        let json = serde_json::to_string(&live_snapshot().await).unwrap();
        let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        let first = digest(&frame_bytes(&json).unwrap());
        for _ in 0..4 {
            assert_eq!(digest(&frame_bytes(&json).unwrap()), first);
        }
    }

    #[test]
    fn a_malformed_snapshot_is_an_error_rather_than_a_panic() {
        assert!(frame_bytes("not json").is_err());
        assert!(frame_bytes("{}").is_err());
        assert!(packet_bytes("[]", true).is_err());
    }

    #[test]
    fn the_host_is_told_the_geometry_rather_than_guessing_it() {
        assert_eq!(panel_geometry(), vec![250, 122, 3904]);
    }
}
