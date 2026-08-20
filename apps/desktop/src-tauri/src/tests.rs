use brickellstatus_eink::ChannelUrgency;

use super::*;

/// A board that has never run our firmware speaks no banner, so the variant
/// written to it is a guess — the first in the bundle, which is an E213 build.
/// Guess wrong and the firmware refuses to drive the panel, e-paper keeps the
/// factory image, and a flash that genuinely succeeded looks like it did
/// nothing. The board settles it at boot, and the app has to act on that.
#[test]
fn a_board_that_is_not_the_panel_we_wrote_asks_for_the_right_build() {
    let mismatched = DeviceBanner::parse("READY INK1 250x122 3904 abc1234 E290 MISMATCH");
    assert_eq!(
        correction_for(&mismatched, "vision-master-e213-v11"),
        Some(PanelModel::E290),
        "an E290 told to run an E213 build must ask for the E290 one"
    );
}

#[test]
fn a_board_running_the_build_it_should_is_left_alone() {
    // No mismatch: nothing to correct, whatever the banner names.
    let happy = DeviceBanner::parse("READY INK1 296x128 4736 abc1234 E290");
    assert_eq!(correction_for(&happy, "vision-master-e290"), None);

    // Mismatch naming the board we already wrote for would mean rewriting the
    // same image forever, so it is not a correction.
    let same = DeviceBanner::parse("READY INK1 296x128 4736 abc1234 E290 MISMATCH");
    assert_eq!(correction_for(&same, "vision-master-e290"), None);

    // A banner with no board named settles nothing.
    let nameless = DeviceBanner::parse("READY INK1 250x122 3904 abc1234 NONE MISMATCH");
    assert_eq!(correction_for(&nameless, "vision-master-e213"), None);
}

#[test]
fn display_status_contract_matches_frontend() {
    let status = DisplayConnectionStatus {
        state: DisplayConnectionState::Connected,
        transport: Some(DisplayConnectionTransport::Ble),
        device_name: Some("BrickellStatus 26B4".into()),
        detail: "ACK INK1".into(),
        last_frame_at: Some("2026-08-14T15:04:05Z".into()),
        last_ack_at: Some("2026-08-14T15:04:05Z".into()),
        panel: Some(PanelModel::E290),
    };
    let value = serde_json::to_value(status).unwrap();
    assert_eq!(value["state"], "connected");
    assert_eq!(value["transport"], "ble");
    assert_eq!(value["deviceName"], "BrickellStatus 26B4");
    assert_eq!(value["lastAckAt"], "2026-08-14T15:04:05Z");
    // The panel travels with the status so the interface can name what was
    // detected instead of naming the board this project started with.
    assert_eq!(value["panel"], "e290");
}

#[test]
fn aisstream_status_contract_matches_frontend_without_secret_material() {
    let status = AisStreamStatus {
        configured: true,
        enabled: true,
        state: AisStreamSourceState::Live,
        detail: "Live AISStream subscription · 2 fresh vessels in range".into(),
        last_position_at: Some("2026-08-14T15:04:05Z".into()),
        vessels_in_range: Some(2),
    };
    let value = serde_json::to_value(status).unwrap();
    assert_eq!(value["state"], "live");
    assert_eq!(value["lastPositionAt"], "2026-08-14T15:04:05Z");
    assert_eq!(value["vesselsInRange"], 2);
    assert!(!value.to_string().contains("api_key"));
}

#[test]
fn fresh_display_controller_never_auto_connects_to_an_espressif_candidate() {
    let display = DisplayController::new(&AppPreferences::default());
    assert!(!display.automatic_reconnect_enabled());
    assert!(!display.delivery_armed());
}

#[test]
fn exact_saved_usb_route_is_armed_for_restart_reconnect() {
    let mut preferences = AppPreferences::default();
    preferences.display.transport = DisplayTransport::Usb;
    preferences.display.serial_port = "/dev/cu.usbmodem14B4201".into();
    let display = DisplayController::new(&preferences);
    assert!(display.automatic_reconnect_enabled());
    assert!(!display.delivery_armed());
}

/// A USB display that has been opened but has not carried a frame yet, which is
/// what the display worker holds between connecting and proving the route.
fn connected_usb_display() -> ActiveDisplay {
    ActiveDisplay::Usb {
        name: "E213 on /dev/cu.usbmodem14B4201".into(),
        transport: Arc::new(UsbTransport::new(UsbConfig {
            port: Some("/dev/cu.usbmodem14B4201".into()),
            ..UsbConfig::default()
        })),
        ready_observed: false,
    }
}

/// The evidence the firmware decision runs on is read from the live route, not
/// assumed. Every state below is produced by the same calls the delivery path
/// makes, so a rename or a rewiring of the send path fails here rather than
/// silently reporting a board as unproven forever.
#[tokio::test]
async fn route_evidence_is_absent_until_a_usb_display_is_actually_connected() {
    let display = DisplayController::new(&AppPreferences::default());
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Absent,
        "with no display open, nothing is going to answer for the board"
    );
}

#[tokio::test]
async fn an_open_usb_route_is_pending_until_a_frame_is_answered() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Pending,
        "an open route that has not been tried says nothing either way yet"
    );
}

#[tokio::test]
async fn an_acknowledged_frame_proves_the_route() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    display.note_frame_acknowledged();
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Acknowledged
    );
}

#[tokio::test]
async fn one_refused_frame_is_not_enough_to_condemn_a_board() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    display.note_frame_unanswered();
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Pending,
        "a single dropped frame must not be read as the wrong firmware"
    );
}

#[tokio::test]
async fn refusals_in_a_row_are_what_condemn_a_board() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    for _ in 0..UNANSWERED_FRAMES_BEFORE_BLAME {
        display.note_frame_unanswered();
    }
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Failing
    );
}

#[tokio::test]
async fn an_acknowledgement_clears_the_refusals_before_it() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    for _ in 0..UNANSWERED_FRAMES_BEFORE_BLAME {
        display.note_frame_unanswered();
    }
    display.note_frame_acknowledged();
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Acknowledged,
        "a board that recovers is not still being judged on frames it dropped"
    );
}

/// The converse, and the reason refusals are read before the acknowledgement: a
/// board that answered once and then stopped answering has died since, and an
/// acknowledgement kept from earlier in the session must not be able to speak
/// for it. Otherwise the route calls itself healthy while the panel sits frozen,
/// and the flash prompt can never be reached again without a relaunch.
#[tokio::test]
async fn a_board_that_dies_after_answering_stops_counting_as_proven() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(connected_usb_display());
    display.note_frame_acknowledged();
    for _ in 0..UNANSWERED_FRAMES_BEFORE_BLAME {
        display.note_frame_unanswered();
    }
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Failing,
        "an acknowledgement from earlier cannot vouch for a board that stopped"
    );
}

#[tokio::test]
async fn a_bluetooth_display_says_nothing_about_the_board_on_usb() {
    let display = DisplayController::new(&AppPreferences::default());
    *display.active.write().await = Some(ActiveDisplay::Ble {
        name: "BrickellStatus 26B4".into(),
        transport: Arc::new(BleTransport::new(BleConfig::default())),
    });
    display.note_frame_acknowledged();
    assert_eq!(
        display.usb_route_evidence().await,
        firmware::RouteEvidence::Absent,
        "frames acknowledged over Bluetooth prove nothing about a USB board"
    );
}

/// A flash ends in a hard reset, so the identity read afterwards describes a
/// port that was vacant a moment earlier. The reading taken while the board was
/// still sitting there is the one the record is keyed to.
#[tokio::test]
async fn the_board_identity_read_before_the_write_is_the_one_recorded() {
    let recorded = board_identity_for_record(Some("F0:9E:9E:3B:26:B4".into()), || async {
        Some("00:00:00:00:00:00".into())
    })
    .await;
    assert_eq!(
        recorded.as_deref(),
        Some("F0:9E:9E:3B:26:B4"),
        "a board that re-enumerated must not overwrite the identity we wrote to"
    );
}

/// ...and asking again is only for the reading that came back empty, which is
/// otherwise a board nothing remembers flashing — and so a board that gets
/// offered the same flash again on the next launch.
#[tokio::test]
async fn an_identity_missed_before_the_write_is_asked_for_again_after_it() {
    let recorded =
        board_identity_for_record(None, || async { Some("F0:9E:9E:3B:26:B4".into()) }).await;
    assert_eq!(recorded.as_deref(), Some("F0:9E:9E:3B:26:B4"));
}

#[tokio::test]
async fn a_board_that_never_reports_an_identity_is_recorded_as_none() {
    assert_eq!(
        board_identity_for_record(None, || async { None }).await,
        None
    );
}

fn usb_preferences() -> AppPreferences {
    let mut preferences = AppPreferences::default();
    preferences.display.transport = DisplayTransport::Usb;
    preferences.display.serial_port = "/dev/cu.usbmodem14B4201".into();
    preferences
}

/// The reported bug, at the level it actually happened: flashing released the
/// serial port and parked automatic reconnect, and only put it back when a
/// display had been connected beforehand. A flash offered *because* nothing was
/// talking to the board is exactly the case where nothing was connected — so
/// the freshly flashed board sat on its boot screen until the app was
/// relaunched, which is the only thing that cleared the park.
///
/// Driven through the real bracket rather than through a hand-made
/// release/restore pair, because the defect was never in either half: it was in
/// a caller that ran one and skipped the other.
#[tokio::test(start_paused = true)]
async fn flashing_hands_the_port_back_even_when_nothing_was_connected() {
    let preferences = usb_preferences();
    let display = DisplayController::new(&preferences);

    let parked_during_write = display
        .holding_the_port_for_flash(&preferences, async {
            // Observed from inside the write: the port must stay parked for as
            // long as espflash is driving the bootloader, or a reconnect that
            // wins the port mid-write leaves a half-written board.
            !display.automatic_reconnect_enabled()
        })
        .await;

    assert!(parked_during_write, "the write must own the port alone");
    assert!(
        display.automatic_reconnect_enabled(),
        "a flashed board must be reconnectable without relaunching the app"
    );
}

/// ...and the same holds for the path that already worked, so the fix cannot be
/// read as having moved the problem from one branch to the other.
#[tokio::test(start_paused = true)]
async fn flashing_hands_the_port_back_when_a_display_was_connected() {
    let preferences = usb_preferences();
    let display = DisplayController::new(&preferences);
    *display.active.write().await = Some(connected_usb_display());

    display
        .holding_the_port_for_flash(&preferences, async {})
        .await;
    assert!(display.automatic_reconnect_enabled());
}

/// A failed write must not cost the display either. This is the one that turns
/// one problem into two: the flash did not take *and* the panel goes quiet.
#[tokio::test(start_paused = true)]
async fn a_failed_flash_still_hands_the_port_back() {
    let preferences = usb_preferences();
    let display = DisplayController::new(&preferences);

    let outcome: Result<(), &str> = display
        .holding_the_port_for_flash(&preferences, async { Err("device or resource busy") })
        .await;

    assert!(outcome.is_err());
    assert!(
        display.automatic_reconnect_enabled(),
        "a board that refused the write is still a board worth talking to"
    );
}

#[test]
fn aisstream_key_shape_is_bounded_and_control_free() {
    assert!(aisstream_key_shape_valid("12345678"));
    assert!(!aisstream_key_shape_valid("short"));
    assert!(!aisstream_key_shape_valid(" 12345678"));
    assert!(!aisstream_key_shape_valid("1234\n5678"));
    assert!(!aisstream_key_shape_valid(&"x".repeat(513)));
}

#[tokio::test]
async fn local_secret_store_round_trips_without_os_vault_access() {
    let directory =
        std::env::temp_dir().join(format!("brickellstatus-secret-test-{}", Uuid::now_v7()));
    let path = directory.join("credentials.json");
    let store = LocalSecretStore::new(path.clone());

    store
        .store_whatsapp_token("local-test-token".into())
        .await
        .unwrap();
    assert_eq!(
        store.whatsapp_token().await.unwrap().as_deref(),
        Some("local-test-token")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    store.delete_whatsapp_token().await.unwrap();
    assert_eq!(store.whatsapp_token().await.unwrap(), None);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_credentials_rest_encrypted_and_migrate_from_plaintext() {
    let directory =
        std::env::temp_dir().join(format!("brickellstatus-secret-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("credentials.json");

    // A pre-DPAPI plaintext file still loads…
    std::fs::write(&path, br#"{"whatsappToken":"legacy-token"}"#).unwrap();
    let store = LocalSecretStore::new(path.clone());
    assert_eq!(
        store.whatsapp_token().await.unwrap().as_deref(),
        Some("legacy-token")
    );

    // …and the next write re-envelopes the whole file without losing fields.
    store
        .store_aisstream_key("stream-key-value".into())
        .await
        .unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains("dpapiCiphertext"),
        "credentials must rest as a DPAPI envelope"
    );
    assert!(
        !raw.contains("legacy-token") && !raw.contains("stream-key-value"),
        "secrets must not rest in plaintext"
    );
    assert_eq!(
        store.whatsapp_token().await.unwrap().as_deref(),
        Some("legacy-token")
    );
    assert_eq!(
        store.aisstream_key().await.unwrap().as_deref(),
        Some("stream-key-value")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn a_sealed_file_this_account_cannot_open_does_not_lock_the_store() {
    let directory =
        std::env::temp_dir().join(format!("brickellstatus-secret-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("credentials.json");
    // Shaped like an envelope this machine did not write, as a file restored
    // from a backup or carried from another account would be.
    std::fs::write(
        &path,
        br#"{"dpapiCiphertext":"bm90LWEtcmVhbC1kcGFwaS1ibG9i"}"#,
    )
    .unwrap();
    let store = LocalSecretStore::new(path.clone());

    // Reading reports no secrets rather than an error...
    assert_eq!(store.whatsapp_token().await.unwrap(), None);

    // ...and writing still succeeds, which it cannot if reading errors first.
    store
        .store_whatsapp_token("replacement-token".into())
        .await
        .unwrap();
    assert_eq!(
        store.whatsapp_token().await.unwrap().as_deref(),
        Some("replacement-token")
    );
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains("dpapiCiphertext") && !raw.contains("replacement-token"),
        "the recovered file must be re-sealed, not left plaintext"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn credential_removal_gates_survive_a_runtime_restart() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.phone_number_id = "123456789".into();
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.token_configured = true;
    preferences.ais.enabled = true;
    preferences.ais.api_key_configured = true;
    engine.save_preferences(preferences.clone()).await.unwrap();

    park_whatsapp_before_secret_delete(&mut preferences);
    park_aisstream_before_secret_delete(&mut preferences);
    engine.save_preferences(preferences).await.unwrap();
    drop(engine);

    let restarted = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let preferences = restarted.get_preferences().await;
    assert!(!preferences.whatsapp.enabled);
    assert!(!preferences.whatsapp.token_configured);
    assert!(!preferences.ais.enabled);
    assert!(!preferences.ais.api_key_configured);
}

#[test]
fn tray_badges_expose_connection_state() {
    let connected = DisplayConnectionStatus {
        state: DisplayConnectionState::Connected,
        transport: Some(DisplayConnectionTransport::Usb),
        ..DisplayConnectionStatus::default()
    };
    assert_eq!(connected.tray_badge(), "•USB");
    assert_eq!(DisplayConnectionStatus::default().tray_badge(), "○");
}

#[test]
fn menu_details_are_single_line_and_bounded() {
    let input = format!("first\nsecond {}", "x".repeat(140));
    let cleaned = clean_menu_text(&input);
    assert!(!cleaned.contains('\n'));
    assert!(cleaned.chars().count() <= 97);
    assert!(cleaned.ends_with('…'));
}

#[test]
fn basis_points_are_rounded_and_bounded() {
    assert_eq!(bps_to_percent(8_249), 82);
    assert_eq!(bps_to_percent(10_000), 100);
    assert_eq!(bps_to_percent(u16::MAX), 100);
}

#[tokio::test]
async fn whatsapp_test_copy_can_never_be_mistaken_for_a_live_alert() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let preferences = engine.get_preferences().await;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Open;
    snapshot.decision.subject = "BRIDGE OPEN".into();
    snapshot.decision.meaning = "Traffic is stopped.".into();
    snapshot.decision.action = "Divert now.".into();

    let request = delivery_test_request(&preferences, &snapshot);
    assert_eq!(request.reason, DeliveryReason::Test);
    assert!(request.notice.subject.starts_with("TEST ONLY"));
    assert_eq!(request.notice.state, NoticeState::Unknown);
    assert!(
        request
            .notice
            .road_meaning
            .contains("does not report a live")
    );
    assert!(request.notice.action.starts_with("No action required"));
    assert!(request.notice.eta.is_none());
    assert!(request.notice.confidence_percent.is_none());
    assert!(!request.notice.subject.contains("BRIDGE OPEN"));
    assert!(!request.notice.road_meaning.contains("Traffic is stopped"));
    assert!(!request.notice.action.contains("Divert now"));
}

/// The old rule collapsed every digit to `#`, so "rain 62% in 40 min" and
/// "rain 95% in 5 min" were the same material and the second was dropped.
/// Escalating rain could never re-alert. Bands replace it: the numbers still
/// have to move meaningfully, but meaningful movement now gets through.
#[tokio::test]
async fn escalating_weather_re_alerts_while_jitter_does_not() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let mut snapshot = engine.get_snapshot().await.unwrap();
    let index = snapshot
        .channels
        .iter()
        .position(|channel| channel.kind == ChannelKindDto::Weather)
        .unwrap();
    snapshot.channels[index].active = true;
    snapshot.channels[index].signal = Some(brickellstatus_runtime::ChannelSignalDto {
        headline: "Rain".into(),
        detail: "Rain 62% in 40 min".into(),
        action: "Forecast conditions cross the configured weather thresholds.".into(),
        severity: Some("Heads-up".into()),
        expires_at: None,
        band: None,
        imminence_minutes: None,
        series: Vec::new(),
        previous_close: None,
    });

    let identity = |snapshot: &AppSnapshot, band: &str| {
        let mut snapshot = snapshot.clone();
        snapshot.channels[index].signal.as_mut().unwrap().band = Some(band.into());
        material_channel_state(&snapshot.channels[index], &snapshot)
    };
    let identity = |band: &str| identity(&snapshot, band);
    let baseline = identity("rain-probability:31-60:p60-79");

    // Same forecast, refreshed: the numbers wobble, the band does not.
    assert_eq!(baseline, identity("rain-probability:31-60:p60-79"));
    // Closer and likelier. Nearer alone is enough; so is likelier alone.
    assert_ne!(baseline, identity("rain-probability:0-5:p95+"));
    assert_ne!(baseline, identity("rain-probability:0-5:p60-79"));
    assert_ne!(baseline, identity("rain-probability:31-60:p95+"));
    // A different rule reaching the same bins is not the same material.
    assert_ne!(baseline, identity("rain-amount:31-60:p60-79"));
    // And a market swinging through zero is always news at equal magnitude.
    assert_ne!(identity("up:2-5"), identity("down:2-5"));
}

#[tokio::test]
async fn sourced_signal_copy_reaches_message_native_and_epaper_surfaces() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let preferences = engine.get_preferences().await;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    let official_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.kind == ChannelKindDto::Official)
        .unwrap();
    let official = &mut snapshot.channels[official_index];
    official.enabled = true;
    official.active = true;
    official.availability = AvailabilityDto::Fresh;
    official.coverage_complete = true;
    official.interrupt_preset = InterruptPreset::Recommended;
    official.summary = "Miami · 1 active official alert".into();
    official.signal = Some(brickellstatus_runtime::ChannelSignalDto {
        headline: "Flash Flood Warning".into(),
        detail: "Life-threatening flash flooding is occurring in downtown Miami.".into(),
        action: "Move to higher ground now.".into(),
        severity: Some("Extreme".into()),
        band: None,
        imminence_minutes: None,
        series: Vec::new(),
        previous_close: None,
        expires_at: Some("2026-08-15T02:00:00Z".into()),
    });

    let request = delivery_request_for_channel(
        &preferences,
        &snapshot,
        &snapshot.channels[official_index],
        true,
        Uuid::now_v7(),
        Uuid::now_v7(),
        1,
        1_786_741_200_000,
        "fixture-material",
    );
    assert_eq!(request.notice.subject, "Flash Flood Warning");
    assert!(request.notice.road_meaning.contains("Life-threatening"));
    assert_eq!(request.notice.action, "Move to higher ground now.");
    assert!(
        request
            .notice
            .evidence
            .iter()
            .any(|item| item == "Severity: Extreme")
    );

    let (title, body) = desktop_notification_copy(&snapshot.channels[official_index], true);
    assert!(title.contains("Flash Flood Warning"));
    assert!(body.contains("Move to higher ground now."));

    // Urgency is decided once in the engine and carried on the snapshot; the
    // card renders it rather than re-deriving it from severity text.
    snapshot.channels[official_index].priority.urgency = UrgencyDto::Emergency;
    let card = channel_card(&snapshot.channels[official_index], &preferences, &snapshot);
    assert_eq!(card.headline, "Flash Flood Warning");
    assert!(card.detail.contains("Life-threatening"));
    assert_eq!(card.action, "Move to higher ground now.");
    assert_eq!(card.urgency, ChannelUrgency::Critical);

    let first_material = delivered_signal_material(&snapshot.channels[official_index]);
    snapshot.channels[official_index].material_key = "unrelated-secondary-change".into();
    assert_eq!(
        first_material,
        delivered_signal_material(&snapshot.channels[official_index]),
        "a secondary item change must not resend identical delivered copy"
    );
}

#[tokio::test]
async fn critical_quiet_bypass_uses_event_severity_not_interrupt_preset() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = true;
    preferences.profile.quiet_hours.start = "00:00".into();
    preferences.profile.quiet_hours.end = "00:00".into();
    preferences.profile.quiet_hours.bypass_emergency = true;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    let official_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.kind == ChannelKindDto::Official)
        .unwrap();
    let official = &mut snapshot.channels[official_index];
    official.enabled = true;
    official.active = true;
    official.interrupt_preset = InterruptPreset::ConfirmedOnly;
    official.signal = Some(brickellstatus_runtime::ChannelSignalDto {
        headline: "Tornado Warning".into(),
        detail: "A tornado warning is active.".into(),
        action: "Take shelter now.".into(),
        severity: Some("Severe".into()),
        expires_at: None,
        band: None,
        imminence_minutes: None,
        series: Vec::new(),
        previous_close: None,
    });

    // Severe is Action, not Emergency, so quiet hours still hold it.
    snapshot.channels[official_index].priority.urgency = UrgencyDto::Action;
    assert!(
        quiet_hours_block(&preferences, &snapshot.channels[official_index], &snapshot).unwrap(),
        "a preset name must not promote Action to the Emergency bypass"
    );
    // Only the engine calling it an Emergency opens the bypass. What earns that
    // classification is tested in the engine, beside the facts it reads.
    snapshot.channels[official_index].priority.urgency = UrgencyDto::Emergency;
    assert!(
        !quiet_hours_block(&preferences, &snapshot.channels[official_index], &snapshot).unwrap()
    );
}

#[tokio::test]
async fn midnight_wrapping_quiet_hours_use_the_snapshot_clock() {
    let engine = RuntimeEngine::new(Store::in_memory().await.unwrap(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = true;
    preferences.profile.quiet_hours.start = "22:00".into();
    preferences.profile.quiet_hours.end = "06:30".into();
    preferences.profile.quiet_hours.time_zone = "America/New_York".into();
    preferences.profile.quiet_hours.bypass_emergency = false;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    let channel = snapshot.channels[0].clone();

    snapshot.generated_at = "2026-08-16T03:00:00Z".into();
    assert!(quiet_hours_block(&preferences, &channel, &snapshot).unwrap());

    snapshot.generated_at = "2026-08-16T11:00:00Z".into();
    assert!(!quiet_hours_block(&preferences, &channel, &snapshot).unwrap());
}

#[tokio::test]
async fn partial_coverage_never_resolves_an_announced_incident() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let bridge = &mut snapshot.channels[bridge_index];
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let mut tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    tracker
        .channels
        .get_mut("bridge.brickell")
        .unwrap()
        .announced = true;
    store
        .set_json(DISPATCH_TRACKER_KEY, &tracker, "2026-08-14T22:00:00Z")
        .await
        .unwrap();

    snapshot.channels[bridge_index].active = false;
    snapshot.channels[bridge_index].availability = AvailabilityDto::Delayed;
    snapshot.channels[bridge_index].coverage_complete = false;
    snapshot.decision.state = BridgeStateDto::Clear;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();

    assert_eq!(store.list_outbox_history(10).await.unwrap().len(), 1);
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    assert!(tracker.channels["bridge.brickell"].active);

    // Complete coverage establishes the resolution, but quiet hours hold
    // it without advancing the tracker. Once quiet hours end, exactly one
    // all-clear is persisted.
    snapshot.channels[bridge_index].availability = AvailabilityDto::Fresh;
    snapshot.channels[bridge_index].coverage_complete = true;
    preferences.profile.quiet_hours.enabled = true;
    preferences.profile.quiet_hours.start = "00:00".into();
    preferences.profile.quiet_hours.end = "00:00".into();
    preferences.profile.quiet_hours.bypass_emergency = false;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    assert_eq!(store.list_outbox_history(10).await.unwrap().len(), 1);
    assert!(
        store
            .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
            .await
            .unwrap()
            .unwrap()
            .channels["bridge.brickell"]
            .active
    );

    preferences.profile.quiet_hours.enabled = false;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    assert_eq!(store.list_outbox_history(10).await.unwrap().len(), 2);
    assert!(
        !store
            .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
            .await
            .unwrap()
            .unwrap()
            .channels["bridge.brickell"]
            .active
    );
}

#[tokio::test]
async fn material_channel_is_persisted_and_leased_from_outbox() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    preferences.whatsapp.recipient = "+13055550123".into();
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let lease = store
        .lease_next("9999-01-01T00:00:00Z", "9999-01-01T00:01:00Z")
        .await
        .unwrap()
        .expect("material transition should create a due outbox row");
    assert_eq!(lease.route_id, WHATSAPP_ROUTE_ID);
    let request: DeliveryRequest = serde_json::from_str(&lease.request_json).unwrap();
    assert_eq!(request.destination.id, "bridge.brickell");
    assert!(matches!(
        request.destination.messaging_consent,
        MessagingConsent::OptedIn { .. }
    ));
}

#[tokio::test]
async fn background_pair_schedules_alert_work_while_display_never_resolves() {
    let (dispatch_started_tx, dispatch_started_rx) = tokio::sync::oneshot::channel();
    let (display_task, dispatch_task) =
        spawn_background_pair(std::future::pending::<()>(), async move {
            let _ = dispatch_started_tx.send(());
        });
    tokio::time::timeout(Duration::from_millis(250), dispatch_started_rx)
        .await
        .expect("the production worker pair must schedule dispatch independently")
        .expect("dispatch worker should report that it ran");
    // Receipt of the dispatch signal while the display future remains
    // permanently pending is the regression proof; Tauri's portable
    // JoinHandle intentionally exposes no Tokio-only `is_finished` API.
    display_task.abort();
    dispatch_task.abort();
}

#[tokio::test]
async fn whatsapp_dispatch_context_reloads_route_after_a_recipient_mutation() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let mut first = engine.get_preferences().await;
    first.whatsapp.recipient = "+13055550111".into();
    engine.save_preferences(first.clone()).await.unwrap();

    // This models the stale clone a worker may have obtained before it
    // waited for the same dispatch lock used by the settings command.
    let stale = engine.get_preferences().await;
    let mut replacement = stale.clone();
    replacement.whatsapp.recipient = "+13055550222".into();
    replacement.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::NotRecorded;
    replacement.whatsapp.consent_recipient = None;
    replacement.whatsapp.consent_recorded_at_millis = None;
    engine.save_preferences(replacement).await.unwrap();

    let (current, _) = current_whatsapp_dispatch_context(&engine).await.unwrap();
    assert_eq!(stale.whatsapp.recipient, "+13055550111");
    assert_eq!(current.whatsapp.recipient, "+13055550222");
    assert!(!whatsapp_consent_is_current(&current.whatsapp));
}

#[tokio::test]
async fn stale_or_changed_outbox_notice_fails_closed_before_delivery() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    {
        let bridge = &mut snapshot.channels[bridge_index];
        bridge.enabled = true;
        bridge.active = true;
        bridge.availability = AvailabilityDto::Fresh;
        bridge.interrupt_preset = InterruptPreset::Recommended;
        if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
            bridge.destinations.push(DestinationIdDto::Whatsapp);
        }
    }
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    let lease = store
        .lease_next("9999-01-01T00:00:00Z", "9999-01-01T00:01:00Z")
        .await
        .unwrap()
        .unwrap();
    let request: DeliveryRequest = serde_json::from_str(&lease.request_json).unwrap();
    let bridge = &snapshot.channels[bridge_index];
    assert!(
        validate_current_outbox(
            &lease,
            &request,
            &tracker,
            bridge,
            &preferences,
            &snapshot,
            request.created_at_millis,
        )
        .is_ok()
    );

    snapshot.channels[bridge_index].active = false;
    snapshot.decision.state = BridgeStateDto::Clear;
    assert!(
        validate_current_outbox(
            &lease,
            &request,
            &tracker,
            &snapshot.channels[bridge_index],
            &preferences,
            &snapshot,
            request.created_at_millis,
        )
        .unwrap_err()
        .contains("changed")
    );

    snapshot.channels[bridge_index].active = true;
    snapshot.decision.state = BridgeStateDto::Likely;
    let max_age = i64::from(
        preferences
            .profile
            .channels
            .iter()
            .find(|channel| channel.id == "bridge.brickell")
            .unwrap()
            .max_age_minutes,
    ) * 60_000;
    assert!(
        validate_current_outbox(
            &lease,
            &request,
            &tracker,
            &snapshot.channels[bridge_index],
            &preferences,
            &snapshot,
            request.created_at_millis + max_age + 1,
        )
        .unwrap_err()
        .contains("freshness limit")
    );
}

#[tokio::test]
async fn all_clear_requires_a_prior_provider_accepted_active_notice() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let bridge = &mut snapshot.channels[bridge_index];
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();

    snapshot.channels[bridge_index].active = false;
    snapshot.decision.state = BridgeStateDto::Clear;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "an unseen active notice must not get an all-clear"
    );
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    let current = tracker.channels.get("bridge.brickell").unwrap();
    assert!(!current.active);
    assert!(!current.announced);
}

#[tokio::test]
async fn accepted_outbox_revision_recovers_announced_state_before_all_clear() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let bridge = &mut snapshot.channels[bridge_index];
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let lease = store
        .lease_next("9999-01-01T00:00:00Z", "9999-01-01T00:01:00Z")
        .await
        .unwrap()
        .unwrap();
    store
        .mark_outbox(
            &lease.id,
            "accepted",
            "2026-08-14T22:00:00Z",
            Some("wamid.fixture"),
            None,
            None,
        )
        .await
        .unwrap();

    // Simulate interruption after provider acceptance but before an older
    // build could record `announced=true` in its separate tracker write.
    snapshot.channels[bridge_index].active = false;
    snapshot.decision.state = BridgeStateDto::Clear;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();

    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].action, "resolved");
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    let current = &tracker.channels["bridge.brickell"];
    assert!(!current.active);
    assert!(current.announced);
}

#[tokio::test]
async fn expired_current_notice_rearms_one_fresh_representation() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let lease = store
        .lease_next("9999-01-01T00:00:00Z", "9999-01-01T00:01:00Z")
        .await
        .unwrap()
        .unwrap();
    let request: DeliveryRequest = serde_json::from_str(&lease.request_json).unwrap();
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    let max_age = i64::from(
        preferences
            .profile
            .channels
            .iter()
            .find(|channel| channel.id == "bridge.brickell")
            .unwrap()
            .max_age_minutes,
    ) * 60_000;
    let expired_at = request.created_at_millis + max_age + 1;
    let reason = validate_current_outbox(
        &lease,
        &request,
        &tracker,
        snapshot
            .channels
            .iter()
            .find(|channel| channel.id == "bridge.brickell")
            .unwrap(),
        &preferences,
        &snapshot,
        expired_at,
    )
    .unwrap_err();
    suppress_and_rearm_current_outbox(
        &store,
        &lease,
        &request,
        &tracker,
        &iso_at(expired_at).unwrap(),
        &reason,
    )
    .await
    .unwrap();

    let rearmed = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    assert!(rearmed.channels["bridge.brickell"].active);
    assert!(rearmed.channels["bridge.brickell"].last_material.is_none());
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.status == "pending"));
    assert!(rows.iter().any(|row| row.status == "suppressed"));
}

#[tokio::test]
async fn temporarily_unavailable_active_notice_rearms_after_source_recovers() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let bridge = &mut snapshot.channels[bridge_index];
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let lease = store
        .lease_next("9999-01-01T00:00:00Z", "9999-01-01T00:01:00Z")
        .await
        .unwrap()
        .unwrap();
    let request: DeliveryRequest = serde_json::from_str(&lease.request_json).unwrap();

    // A temporary source outage makes the queued copy unverifiable, but
    // does not establish a resolution or superseding material revision.
    snapshot.channels[bridge_index].availability = AvailabilityDto::Offline;
    snapshot.channels[bridge_index].coverage_complete = false;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    let reason = validate_current_outbox(
        &lease,
        &request,
        &tracker,
        &snapshot.channels[bridge_index],
        &preferences,
        &snapshot,
        request.created_at_millis,
    )
    .unwrap_err();
    assert_eq!(
        reason,
        "The channel state changed before this notice could be delivered"
    );
    suppress_and_rearm_current_outbox(
        &store,
        &lease,
        &request,
        &tracker,
        "2026-08-14T22:00:00Z",
        &reason,
    )
    .await
    .unwrap();

    snapshot.channels[bridge_index].availability = AvailabilityDto::Fresh;
    snapshot.channels[bridge_index].coverage_complete = true;
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.status == "suppressed"));
    assert!(rows.iter().any(|row| row.status == "pending"));
}

#[tokio::test]
async fn recipient_route_fingerprint_never_inherits_announced_state() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    let first_fingerprint = whatsapp_route_fingerprint(&preferences).unwrap();
    let mut tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    tracker
        .channels
        .get_mut("bridge.brickell")
        .unwrap()
        .announced = true;
    store
        .set_json(DISPATCH_TRACKER_KEY, &tracker, "2026-08-14T22:00:00Z")
        .await
        .unwrap();

    // Model the retry path after preferences were saved but route cleanup
    // failed: the new recipient is live while the old tracker remains.
    preferences.whatsapp.recipient = "+13055550999".into();
    preferences.whatsapp.consent_recipient = Some("+13055550999".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_260_000);
    let second_fingerprint = whatsapp_route_fingerprint(&preferences).unwrap();
    assert_ne!(first_fingerprint, second_fingerprint);
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();

    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tracker.route_fingerprint.as_deref(),
        Some(second_fingerprint.as_str())
    );
    let current = &tracker.channels["bridge.brickell"];
    assert!(current.active);
    assert!(!current.announced);
    // This test is about routing, not ordering: a changed recipient must get its
    // own warning rather than inheriting the previous one's announced state.
    // Asserting on history[0] made it depend on tie-breaking between two rows
    // written milliseconds apart, which is incidental to what is being proven.
    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2, "the new recipient gets an initial warning");
    let addressed = rows
        .iter()
        .map(|row| {
            serde_json::from_str::<DeliveryRequest>(&row.request_json)
                .unwrap()
                .destination
                .address
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        addressed.contains("+13055550999"),
        "the new recipient must have been written its own notice: {addressed:?}"
    );
    assert!(addressed.contains("+13055550123"));
}

#[tokio::test]
async fn credential_replacement_rearms_an_unchanged_active_alert() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store.clone(), RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.profile.quiet_hours.enabled = false;
    preferences.whatsapp.enabled = true;
    preferences.whatsapp.token_configured = true;
    preferences.whatsapp.recipient = "+13055550123".into();
    preferences.whatsapp.consent = brickellstatus_runtime::WhatsAppRecipientConsent::OptedIn;
    preferences.whatsapp.consent_recipient = Some("+13055550123".into());
    preferences.whatsapp.consent_recorded_at_millis = Some(1_786_741_200_000);
    let mut snapshot = engine.get_snapshot().await.unwrap();
    snapshot.decision.state = BridgeStateDto::Likely;
    let bridge = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    bridge.enabled = true;
    bridge.active = true;
    bridge.availability = AvailabilityDto::Fresh;
    bridge.coverage_complete = true;
    bridge.interrupt_preset = InterruptPreset::Recommended;
    if !bridge.destinations.contains(&DestinationIdDto::Whatsapp) {
        bridge.destinations.push(DestinationIdDto::Whatsapp);
    }

    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();
    suppress_and_reset_whatsapp_route(
        &store,
        "2026-08-14T22:00:00Z",
        "Meta credential was added or replaced",
    )
    .await
    .unwrap();
    enqueue_material_whatsapp_updates(&store, &preferences, &snapshot)
        .await
        .unwrap();

    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.status == "suppressed"));
    assert!(rows.iter().any(|row| row.status == "pending"));
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .unwrap()
        .unwrap();
    assert!(tracker.channels["bridge.brickell"].active);
    assert!(!tracker.channels["bridge.brickell"].announced);
}

#[tokio::test]
async fn interrupt_presets_have_distinct_fail_closed_semantics() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    let bridge_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.id == "bridge.brickell")
        .unwrap();
    snapshot.channels[bridge_index].enabled = true;
    snapshot.channels[bridge_index].active = true;
    snapshot.channels[bridge_index].interrupt_preset = InterruptPreset::ConfirmedOnly;
    snapshot.decision.state = BridgeStateDto::Likely;
    assert!(!interrupt_allows(
        &snapshot.channels[bridge_index],
        &preferences,
        &snapshot
    ));
    snapshot.decision.state = BridgeStateDto::Open;
    assert!(interrupt_allows(
        &snapshot.channels[bridge_index],
        &preferences,
        &snapshot
    ));

    let weather_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.kind == ChannelKindDto::Weather)
        .unwrap();
    snapshot.channels[weather_index].enabled = true;
    snapshot.channels[weather_index].active = true;
    snapshot.channels[weather_index].interrupt_preset = InterruptPreset::ConfirmedOnly;
    assert!(!interrupt_allows(
        &snapshot.channels[weather_index],
        &preferences,
        &snapshot
    ));
    snapshot.channels[weather_index].interrupt_preset = InterruptPreset::Meaningful;
    assert!(interrupt_allows(
        &snapshot.channels[weather_index],
        &preferences,
        &snapshot
    ));
    snapshot.channels[weather_index].interrupt_preset = InterruptPreset::Custom;
    assert!(!interrupt_allows(
        &snapshot.channels[weather_index],
        &preferences,
        &snapshot
    ));

    let news_index = snapshot
        .channels
        .iter()
        .position(|channel| channel.kind == ChannelKindDto::News)
        .unwrap();
    snapshot.channels[news_index].enabled = true;
    snapshot.channels[news_index].active = true;
    snapshot.channels[news_index].interrupt_preset = InterruptPreset::Recommended;
    assert!(!interrupt_allows(
        &snapshot.channels[news_index],
        &preferences,
        &snapshot
    ));
    preferences
        .profile
        .channels
        .iter_mut()
        .find(|channel| channel.id == snapshot.channels[news_index].id)
        .unwrap()
        .scope
        .insert("breakingOnly".into(), serde_json::json!(true));
    assert!(interrupt_allows(
        &snapshot.channels[news_index],
        &preferences,
        &snapshot
    ));
}

#[tokio::test]
async fn rotation_honors_home_cadence_and_surface_presence() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let preferences = engine.get_preferences().await;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    for channel in &mut snapshot.channels {
        channel.enabled = matches!(
            channel.id.as_str(),
            "bridge.brickell" | "weather.miami" | "news.local"
        );
        // Rotation *is* a standing reservation, which is what distinguishes it
        // from ActiveOnly. A channel with nothing urgent still renders its
        // summary, so its slot is not blank -- and requiring material here made
        // the two presences behave identically, leaving a reader who had set
        // every channel to Rotation watching one frame forever.
        channel.active = channel.id != "bridge.brickell";
        channel.destinations = vec![DestinationIdDto::Epaper];
        channel.presence = if channel.id == "bridge.brickell" {
            SurfacePresence::Home
        } else {
            SurfacePresence::Rotation
        };
    }
    // The anchor every third slot, the rest sharing the gaps. News appears here
    // where it previously could not: rotation used to drop News, Official,
    // Hurricane and Earthquake by kind, which made `presence: Rotation`
    // meaningless for exactly the channels worth rotating.
    let slot = |snapshot: &AppSnapshot, index: u64| {
        rotation_channel(snapshot, &preferences, index).map(|channel| channel.id.to_owned())
    };
    assert_eq!(slot(&snapshot, 0).as_deref(), Some("bridge.brickell"));
    assert_eq!(slot(&snapshot, 1).as_deref(), Some("weather.miami"));
    assert_eq!(slot(&snapshot, 2).as_deref(), Some("news.local"));
    assert_eq!(slot(&snapshot, 3).as_deref(), Some("bridge.brickell"));
    assert_eq!(slot(&snapshot, 4).as_deref(), Some("weather.miami"));

    // Quiet is not absent. A Rotation channel keeps its turn with nothing
    // urgent to report, because the card still carries its summary -- today's
    // weather is worth a glance whether or not it crossed an alert threshold.
    let mut quiet = snapshot.clone();
    for channel in &mut quiet.channels {
        channel.active = false;
    }
    assert_eq!(slot(&quiet, 1).as_deref(), Some("weather.miami"));
    assert_eq!(slot(&quiet, 2).as_deref(), Some("news.local"));

    // ActiveOnly is where "wait until there is something" lives, and it still
    // means exactly that. This is the distinction the previous rule erased.
    let mut on_demand = quiet.clone();
    for channel in &mut on_demand.channels {
        if channel.id == "news.local" {
            channel.presence = SurfacePresence::ActiveOnly;
        }
    }
    for index in 0..6 {
        assert_ne!(
            slot(&on_demand, index).as_deref(),
            Some("news.local"),
            "an inactive active-only channel took slot {index}"
        );
    }

    // With every other channel off the panel, the anchor holds it — the one
    // thing always worth reading.
    let mut silent = quiet.clone();
    for channel in &mut silent.channels {
        if channel.id != "bridge.brickell" {
            channel.presence = SurfacePresence::Off;
        }
    }
    for index in 0..4 {
        assert_eq!(slot(&silent, index).as_deref(), Some("bridge.brickell"));
    }
}

#[tokio::test]
async fn off_messages_only_and_inactive_active_only_never_reach_epaper() {
    let store = Store::in_memory().await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let preferences = engine.get_preferences().await;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    for (index, channel) in snapshot.channels.iter_mut().enumerate() {
        channel.enabled = true;
        channel.active = false;
        channel.destinations = vec![DestinationIdDto::Epaper];
        channel.presence = match index % 3 {
            0 => SurfacePresence::Off,
            1 => SurfacePresence::MessagesOnly,
            _ => SurfacePresence::ActiveOnly,
        };
    }
    assert!(rotation_channel(&snapshot, &preferences, 0).is_none());
    let active_only = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.presence == SurfacePresence::ActiveOnly)
        .unwrap();
    active_only.active = true;
    let expected = active_only.id.clone();
    assert_eq!(
        rotation_channel(&snapshot, &preferences, 0).map(|channel| &channel.id),
        Some(&expected)
    );
}

mod river_spans {
    use brickellstatus_runtime::{
        BridgeRelationDto, BridgeStateIntervalDto, ObservedBridgeStateDto,
    };
    use jiff::tz::TimeZone;

    use super::super::{local_clock, span_code, upstream_spans};

    fn interval(
        key: &str,
        state: ObservedBridgeStateDto,
        started_at: &str,
        ended_at: Option<&str>,
    ) -> BridgeStateIntervalDto {
        BridgeStateIntervalDto {
            source_id: "fl511.bridge.brickell".into(),
            bridge_key: key.into(),
            bridge_name: key.to_uppercase(),
            relation: BridgeRelationDto::Upstream,
            river_order: match key {
                "sw_2_ave" => 1,
                "sw_1_st" => 2,
                "w_flagler" => 3,
                _ => 9,
            },
            state,
            started_at: started_at.into(),
            ended_at: ended_at.map(Into::into),
        }
    }

    fn miami() -> TimeZone {
        TimeZone::get("America/New_York").unwrap()
    }

    #[test]
    fn only_an_unfinished_interval_reports_a_span_as_open() {
        let spans = upstream_spans(
            &[
                interval(
                    "sw_2_ave",
                    ObservedBridgeStateDto::Up,
                    "2026-08-15T18:20:00Z",
                    None,
                ),
                // Ended, so it is history. Reporting it as up would tell a
                // driver the river is blocked when it is not.
                interval(
                    "sw_1_st",
                    ObservedBridgeStateDto::Up,
                    "2026-08-15T17:00:00Z",
                    Some("2026-08-15T17:10:00Z"),
                ),
            ],
            Some(&miami()),
        );
        assert_eq!(spans.len(), 2);
        let two_ave = spans.iter().find(|span| span.code == "2AV").unwrap();
        assert!(two_ave.open);
        assert_eq!(two_ave.opened_at.as_deref(), Some("14:20"));
        let first_st = spans.iter().find(|span| span.code == "1ST").unwrap();
        assert!(!first_st.open);
        assert!(first_st.opened_at.is_none());
    }

    #[test]
    fn an_in_progress_interval_beats_a_newer_completed_one() {
        let spans = upstream_spans(
            &[
                interval(
                    "sw_2_ave",
                    ObservedBridgeStateDto::Up,
                    "2026-08-15T18:00:00Z",
                    None,
                ),
                interval(
                    "sw_2_ave",
                    ObservedBridgeStateDto::Down,
                    "2026-08-15T18:05:00Z",
                    Some("2026-08-15T18:06:00Z"),
                ),
            ],
            Some(&miami()),
        );
        assert_eq!(spans.len(), 1);
        assert!(spans[0].open);
    }

    #[test]
    fn the_target_bridge_is_not_listed_among_upstream_spans() {
        let mut target = interval(
            "brickell",
            ObservedBridgeStateDto::Up,
            "2026-08-15T18:00:00Z",
            None,
        );
        target.relation = BridgeRelationDto::Target;
        assert!(upstream_spans(&[target], Some(&miami())).is_empty());
    }

    #[test]
    fn opening_times_resolve_in_the_bridge_zone_not_utc() {
        // 18:20Z in August is 14:20 in Miami.
        assert_eq!(
            local_clock("2026-08-15T18:20:00Z", &miami()).as_deref(),
            Some("14:20")
        );
        // Midnight rollover backwards across the offset.
        assert_eq!(
            local_clock("2026-08-16T02:30:00Z", &miami()).as_deref(),
            Some("22:30")
        );
        assert!(local_clock("not a timestamp", &miami()).is_none());
    }

    #[test]
    fn span_codes_stay_within_the_three_characters_the_panel_allows() {
        assert_eq!(span_code("sw_2_ave", "SW 2 Ave"), "2AV");
        assert_eq!(span_code("sw_1_st", "SW 1 St"), "1ST");
        assert_eq!(span_code("w_flagler", "W Flagler"), "FLG");
        // An unknown span still shows up rather than disappearing.
        let unknown = span_code("some_new_bridge", "Some New Bridge");
        assert!(!unknown.is_empty());
        assert!(unknown.chars().count() <= 3);
    }

    #[test]
    fn a_missing_time_zone_still_reports_the_span_as_open() {
        let spans = upstream_spans(
            &[interval(
                "sw_2_ave",
                ObservedBridgeStateDto::Up,
                "2026-08-15T18:20:00Z",
                None,
            )],
            None,
        );
        assert!(spans[0].open);
        assert!(spans[0].opened_at.is_none());
    }
}

mod panel {
    use super::super::{PanelBroker, PanelSelection, prove_backoff, should_prove_now};
    use super::*;

    async fn epaper_snapshot() -> (AppSnapshot, AppPreferences) {
        let store = Store::in_memory().await.unwrap();
        let engine = RuntimeEngine::new(store, RuntimeConfig::default())
            .await
            .unwrap();
        let preferences = engine.get_preferences().await;
        let mut snapshot = engine.get_snapshot().await.unwrap();
        for channel in &mut snapshot.channels {
            channel.enabled = matches!(channel.id.as_str(), "bridge.brickell" | "weather.miami");
            channel.active = false;
            channel.destinations = vec![DestinationIdDto::Epaper];
            channel.presence = if channel.id == "bridge.brickell" {
                SurfacePresence::Home
            } else {
                SurfacePresence::Rotation
            };
        }
        (snapshot, preferences)
    }

    fn raise(snapshot: &mut AppSnapshot, id: &str, score: u16, key: &str) {
        let channel = snapshot
            .channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .expect("channel");
        channel.active = true;
        channel.interrupt_preset = InterruptPreset::Meaningful;
        channel.priority.score = score;
        channel.material_key = key.into();
    }

    /// Raises a channel that says something is about to happen.
    fn raise_imminent(snapshot: &mut AppSnapshot, id: &str, score: u16, key: &str, minutes: u16) {
        raise(snapshot, id, score, key);
        let channel = snapshot
            .channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .expect("channel");
        channel.priority.imminence_minutes = Some(minutes);
    }

    /// The reported failure, and the flagship one: a bridge predicted to open
    /// in three to eight minutes appeared once, held its forty-five seconds, and
    /// then handed the panel to the rotation for the rest of the window — so a
    /// reader with a bridge about to go up in front of them was shown stock
    /// prices.
    ///
    /// It scores 493, not the 900 re-assertion required, and it is not
    /// `confirmed`, because nothing has been observed yet. That is what
    /// *predicted* means, and predicting is the product.
    #[tokio::test]
    async fn a_bridge_about_to_open_keeps_taking_the_panel_back() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        // Paused only now: the fixture's store needs a real clock to open.
        tokio::time::pause();
        let broker = PanelBroker::default();
        raise_imminent(&mut snapshot, "bridge.brickell", 493, "likely:3-8", 5);

        broker.ingest(&snapshot, &preferences);
        assert!(
            matches!(
                broker.next(&snapshot, &preferences, 0),
                Some(PanelSelection::Alert { ref channel_id, .. }) if channel_id == "bridge.brickell"
            ),
            "the warning has to reach the panel at all"
        );

        // The hold expires and nothing about the bridge has changed: it is
        // still going to open, and still says so.
        tokio::time::advance(PanelBroker::alert_hold() + Duration::from_secs(1)).await;
        broker.ingest(&snapshot, &preferences);
        assert!(
            matches!(
                broker.next(&snapshot, &preferences, 0),
                Some(PanelSelection::Alert { ref channel_id, .. }) if channel_id == "bridge.brickell"
            ),
            "an opening still minutes away must reclaim the panel, not yield it"
        );
    }

    /// ...and the window ends. Once the opening is no longer near, the bridge
    /// stops taking the panel back and the rotation resumes, so this cannot
    /// become the old bug where one channel pinned the display forever.
    #[tokio::test]
    async fn an_event_that_is_no_longer_near_releases_the_panel() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        tokio::time::pause();
        let broker = PanelBroker::default();
        raise_imminent(&mut snapshot, "bridge.brickell", 493, "likely:3-8", 5);
        broker.ingest(&snapshot, &preferences);
        let _ = broker.next(&snapshot, &preferences, 0);

        // The estimate moves out past the warning horizon without otherwise
        // changing, which is what a vessel slowing down looks like.
        raise_imminent(&mut snapshot, "bridge.brickell", 493, "likely:3-8", 45);
        tokio::time::advance(PanelBroker::alert_hold() + Duration::from_secs(1)).await;
        broker.ingest(&snapshot, &preferences);
        assert!(
            matches!(
                broker.next(&snapshot, &preferences, 0),
                Some(PanelSelection::Rotation { .. })
            ),
            "a distant estimate must not keep the panel"
        );
    }

    /// A routine card has no imminence at all and must never reclaim the panel
    /// on this path — it is what the reader was seeing instead of the bridge.
    #[tokio::test]
    async fn a_routine_card_never_reclaims_the_panel() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        tokio::time::pause();
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 120, "markets:flat");
        broker.ingest(&snapshot, &preferences);
        let _ = broker.next(&snapshot, &preferences, 0);

        tokio::time::advance(PanelBroker::alert_hold() + Duration::from_secs(1)).await;
        broker.ingest(&snapshot, &preferences);
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Rotation { .. })
        ));
    }

    #[tokio::test]
    async fn an_alert_preempts_the_rotation() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        let broker = PanelBroker::default();
        broker.ingest(&snapshot, &preferences);
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Rotation { .. })
        ));

        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        broker.ingest(&snapshot, &preferences);
        assert_eq!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Alert {
                channel_id: "weather.miami".into(),
                score: 470
            })
        );
    }

    /// The invariant that protects the anchor. An alert must not eat a rotation
    /// slot, or a burst of them silently skips the bridge's home cadence.
    #[tokio::test]
    async fn an_alert_does_not_consume_a_rotation_slot() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        broker.ingest(&snapshot, &preferences);

        // The alert is served without advancing the index...
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Alert { .. })
        ));
        // ...so slot 0 is still the anchor's when rotation resumes.
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Rotation { channel_id }) if channel_id == "bridge.brickell"
        ));
    }

    #[tokio::test]
    async fn the_same_alert_does_not_re_enter_but_an_escalation_does() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 400, "rain:31-60");
        broker.ingest(&snapshot, &preferences);
        assert!(broker.next(&snapshot, &preferences, 0).is_some());

        // Same condition, same key: still true, not news.
        broker.ingest(&snapshot, &preferences);
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Rotation { .. })
        ));

        // Closer now: a different key, so it interrupts again. This is the
        // behaviour the old digit-collapsing dedup made impossible.
        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        broker.ingest(&snapshot, &preferences);
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Alert { .. })
        ));
    }

    #[tokio::test]
    async fn a_superseded_alert_is_discarded_rather_than_shown() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        broker.ingest(&snapshot, &preferences);

        // The channel goes quiet before the queued alert is served.
        snapshot
            .channels
            .iter_mut()
            .find(|channel| channel.id == "weather.miami")
            .unwrap()
            .active = false;
        assert!(matches!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Rotation { .. })
        ));
    }

    #[tokio::test]
    async fn the_higher_score_is_served_first() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        raise(&mut snapshot, "bridge.brickell", 1005, "bridge:open");
        broker.ingest(&snapshot, &preferences);
        assert_eq!(
            broker.next(&snapshot, &preferences, 0),
            Some(PanelSelection::Alert {
                channel_id: "bridge.brickell".into(),
                score: 1005
            })
        );
    }

    #[tokio::test]
    async fn a_dwell_ends_early_only_for_something_that_outranks_it() {
        let (mut snapshot, preferences) = epaper_snapshot().await;
        // Paused after the store is built: an auto-advancing clock fires sqlx's
        // pool-acquire timeout the instant the runtime looks idle.
        tokio::time::pause();
        let broker = PanelBroker::default();
        raise(&mut snapshot, "weather.miami", 470, "rain:6-15");
        broker.ingest(&snapshot, &preferences);

        // Equal score waits out the full hold rather than trading the panel.
        let started = tokio::time::Instant::now();
        broker
            .wait_or_preempt(470, std::time::Duration::from_secs(45))
            .await;
        assert!(
            tokio::time::Instant::now().duration_since(started)
                >= std::time::Duration::from_secs(45)
        );

        // A higher score cuts it short, but never before the on-screen floor.
        raise(&mut snapshot, "bridge.brickell", 1005, "bridge:open");
        broker.ingest(&snapshot, &preferences);
        let started = tokio::time::Instant::now();
        broker
            .wait_or_preempt(470, std::time::Duration::from_secs(45))
            .await;
        let held = tokio::time::Instant::now().duration_since(started);
        assert!(held >= std::time::Duration::from_secs(8), "held {held:?}");
        assert!(held < std::time::Duration::from_secs(45), "held {held:?}");
    }

    #[tokio::test]
    async fn a_parked_display_keeps_retrying_its_proof_frame() {
        let now = tokio::time::Instant::now();
        // Connected but never acknowledged: prove it rather than waiting for a
        // human to press a button.
        assert!(should_prove_now(true, false, now, now));
        // Already armed, or nothing attached: nothing to do.
        assert!(!should_prove_now(true, true, now, now));
        assert!(!should_prove_now(false, false, now, now));
        // Backing off after failures, then due again.
        assert!(!should_prove_now(
            true,
            false,
            now,
            now + std::time::Duration::from_secs(5)
        ));
        assert!(prove_backoff(4) > prove_backoff(1));
    }
}

mod radar {
    use brickellstatus_collectors::parse_rainviewer_index;

    use super::super::{panel_tile_url, radar_enabled, radar_layer_from_items};
    use super::*;

    const OBSERVED: i64 = 1_786_844_400;

    fn frame_items() -> Vec<brickellstatus_collectors::CollectorItem> {
        let body = serde_json::to_vec(&serde_json::json!({
            "host": "https://tilecache.rainviewer.com",
            "radar": {"past": [{"time": OBSERVED, "path": "/v2/radar/f6ad5f810281"}]},
        }))
        .unwrap();
        parse_rainviewer_index(&body, OBSERVED).unwrap()
    }

    /// The exact shape RainViewer serves, verified against the live endpoint:
    /// `{host}{path}/{size}/{z}/{x}/{y}/{colour}/{smoothing}_{snow}.png`. A
    /// template that is merely plausible produces a map of empty tiles.
    #[test]
    fn the_tile_template_matches_the_shape_maplibre_and_rainviewer_agree_on() {
        let layer =
            radar_layer_from_items(&frame_items(), OBSERVED * 1_000 + 90_000).expect("a layer");
        assert_eq!(
            layer.tile_url_template,
            "https://tilecache.rainviewer.com/v2/radar/f6ad5f810281/512/{z}/{x}/{y}/4/1_0.png"
        );
        // Age is reported so the overlay can say how old it is rather than
        // implying it is live.
        assert_eq!(layer.age_seconds, 90);
        // Credit is mandatory under RainViewer's free terms, in their own
        // wording and pointing at their own link.
        assert!(layer.attribution.contains("Weather data by"));
        assert!(layer.attribution.contains("https://www.rainviewer.com/"));
        // Declared so the map overzooms rather than requesting tiles past the
        // service's documented ceiling.
        assert_eq!(layer.max_zoom, 7);
    }

    /// The panel asks for the same frame at a coordinate instead of at a tile
    /// index — RainViewer serves that directly, which is why no projection
    /// arithmetic exists anywhere in this path. It also asks for the monochrome
    /// scheme: the colour schemes are not luminance ramps, so a rainbow tile
    /// converted to grey inverts the intensity ordering mid-range.
    #[test]
    fn the_panel_asks_for_a_monochrome_tile_centred_on_the_reader() {
        let layer = radar_layer_from_items(&frame_items(), OBSERVED * 1_000).unwrap();
        let url = panel_tile_url(&layer, 25.7699, -80.19005).unwrap();
        assert_eq!(
            url.as_str(),
            "https://tilecache.rainviewer.com/v2/radar/f6ad5f810281/256/7/25.7699/-80.1900/0/1_0.png"
        );
        // A coordinate that cannot be formatted into a request never becomes one.
        assert!(panel_tile_url(&layer, f64::NAN, -80.19).is_none());
        assert!(panel_tile_url(&layer, 25.77, f64::INFINITY).is_none());
    }

    /// One switch governs both surfaces. The map's own toggle only hides the
    /// layer for a session; this decides whether radar is offered at all, and
    /// whether the panel spends a fetch on it.
    #[tokio::test]
    async fn turning_radar_off_stops_it_reaching_either_surface() {
        let store = Store::in_memory().await.unwrap();
        let engine = RuntimeEngine::new(store, RuntimeConfig::default())
            .await
            .unwrap();
        let mut preferences = engine.get_preferences().await;
        assert!(radar_enabled(&preferences), "radar ships on");

        let weather = preferences
            .profile
            .channels
            .iter_mut()
            .find(|channel| channel.kind == ChannelKindDto::Weather)
            .unwrap();
        weather
            .scope
            .insert("radarEnabled".into(), serde_json::json!(false));
        assert!(!radar_enabled(&preferences));
    }

    #[test]
    fn no_frame_means_no_layer_rather_than_a_broken_one() {
        assert!(radar_layer_from_items(&[], OBSERVED * 1_000).is_none());
    }

    /// The console reads camelCase. A silent rename here shows up as a map with
    /// no radar and no error anywhere.
    #[test]
    fn the_layer_crosses_the_bridge_in_the_shape_the_console_reads() {
        let layer = radar_layer_from_items(&frame_items(), OBSERVED * 1_000).unwrap();
        let json = serde_json::to_value(&layer).unwrap();
        assert!(json.get("tileUrlTemplate").is_some());
        assert!(json.get("observedAt").is_some());
        assert!(json.get("ageSeconds").is_some());
        assert!(json.get("attribution").is_some());
    }
}
