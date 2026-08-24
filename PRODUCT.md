# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

People who want a quiet, personally configured signal console for information that is worth interrupting them—starting with people who routinely approach the Brickell Avenue Bridge and need to decide, several minutes before arrival, whether to continue toward it or take another route. The first distribution target is a small trusted group of friends running the system on ordinary computers, with a glanceable companion display on a Heltec ESP32 e-paper board, 2.13-inch or 2.9-inch.

## Product Purpose

Provide a configurable personal signal console whose flagship channel gives an honest advance warning that the Brickell Avenue Bridge is likely to open, using confidence-weighted evidence before road traffic or cameras reveal that the bridge is already opening. Weather, official alerts, hurricanes, news, markets, and future channels use the same inspectable freshness, priority, routing, and interruption rules. Success means users choose what is collected and shown, receive useful low-noise warnings through the surfaces they select, and can always see why something reached them.

## Positioning

This is a user-programmable signal instrument, not a feed firehose or binary bridge-status page. Every channel must turn sourced observations into freshness-aware, deduplicated signals governed by the user's delivery policy. The bridge channel goes further by combining independent evidence into a time-decaying confidence estimate. One policy decision publishes a consistent snapshot to the desktop/web surface, notifications, and physical e-paper display.

## Operating Context

- Users check status shortly before or during a drive and may only have a few seconds to read it.
- Meaningful stages are Clear, Likely, and Open. There is deliberately no middle "something might happen" stage: it fired on evidence too weak to act on, which is what teaches a reader to stop believing the channel. Either the evidence supports saying an opening is likely, or the instrument stays quiet and shows the context without raising urgency. Predictive confidence distinguishes High from Very High without inventing an unobserved intermediate state.
- Legal opening windows are context and a confidence modifier, never proof that an opening will occur.
- AISStream vessel movement and upstream bridge progression are predictive signals; FL511 or equivalent controller status is ground truth.
- WhatsApp is the preferred message channel. USB and Bluetooth are the desired display transports.
- Users choose which channels to watch and configure their places, sources, output connections, and quiet hours. The runtime owns freshness, expiry, slide membership, ordering, cadence, and interruption consistently across channels.
- Every important location is chosen on a reusable global pan/zoom map with search and draggable pins; raw coordinates are an advanced escape hatch, never the primary experience. Device location is an explicit one-shot action and is never sampled passively.
- Every channel has an operational master gate, and independently configurable sub-rules expose their own enable/disable state. A disabled collector does not keep polling merely because its card is hidden.

## Capabilities and Constraints

- Local-first and distributable to a friend without requiring a proprietary hosted backend.
- Sends deduplicated notices for material state changes; does not alert solely because a legal opening slot is approaching.
- Supports pluggable observations so schedule, live AISStream movement, upstream bridge, and ground-truth collectors can mature independently.
- Shows estimate range, confidence, evidence sources, freshness, and next legal slot.
- Drives a Heltec Vision Master E-Paper board (the no-LoRa models: E213 at 250 x 122, E290 at 296 x 128) over USB serial or Bluetooth Low Energy. The board identifies its own panel at boot, so no one is asked which display they own, for showing frames or for flashing firmware.
- Provides guided BLE/USB discovery, direct GATT connection, connection state, and physical `ACK INK1` proof. Closing the configuration window leaves the service running in the platform tray/menu bar until the user explicitly quits.
- Provides an optional AISStream WebSocket source with an operating-system-vault API key, a bridge-target-derived 2–30 km coverage radius, and explicit parked/missing-key/armed/live/degraded health. When enabled, only the backend sends that derived bounding box and key to AISStream over WSS; the UI never connects browser-side or samples location passively.
- Provides first-class configurable channels for the bridge predictor, local weather, official life-safety alerts, hurricane changes, news/RSS, and market watch items; additional channels join through the same typed interface.
- Every relevant item becomes one current notice and one display slide until its typed expiry or resolution. Imminent, high-confidence events rank first and may interrupt once; continuing relevance never creates a repeat count.
- User-authored policy stays structured and inspectable; arbitrary remote code is not required for ordinary configuration.
- Does not depend on UHF hardware.
- Camera and road-traffic detection are explicitly late confirmation sources, not the predictive foundation.
- AISStream and WhatsApp credentials are user-supplied; test fixtures stay isolated from production paths.
- AISStream is a beta/no-SLA predictive source. AIS carriage is incomplete, positions may be absent or delayed, and standard messages do not reliably provide air draft.
- Exact public FL511 event-feed availability remains an integration risk isolated behind an adapter.
- The first macOS distribution is an unsigned DMG using the system WKWebView, with no bundled browser or map tiles and a 25 MiB release-artifact ceiling.

## Evidence on Hand

The founding research supplied in the project brief includes the applicable opening schedule, candidate signal hierarchy, realistic warning targets, and links to eCFR, FL511, AIS carriage rules, and IsBridgeUp. No private API credentials, production event history, customer claims, or guaranteed warning-time benchmark have been supplied and none may be fabricated.

## Product Principles

1. Warn ahead, confirm later.
2. Confidence must be explainable and decay with stale evidence.
3. One event model feeds every surface and transport.
4. The user owns the signal mix; silence is better than false urgency or an unwanted feed.
5. Local operation, inspectable data, and graceful degradation are default behavior.
6. Relevance is membership: if an item is current it is present, and when it passes it is gone.

## Accessibility & Inclusion

Critical states cannot rely on color alone. The web surface must support keyboard navigation, reduced motion, high contrast, readable touch targets, and concise screen-reader announcements. The e-paper layout must remain legible at a glance in bright daylight and express state with words, hierarchy, and simple shapes.

> Product facts above are derived directly from the initial brief. The working product name remains open; current providers and their limits are named explicitly.
