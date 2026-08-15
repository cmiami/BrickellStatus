use super::*;

#[test]
fn display_status_contract_matches_frontend() {
    let status = DisplayConnectionStatus {
        state: DisplayConnectionState::Connected,
        transport: Some(DisplayConnectionTransport::Ble),
        device_name: Some("InkDock E213".into()),
        detail: "ACK INK1".into(),
        last_frame_at: Some("2026-08-14T15:04:05Z".into()),
        last_ack_at: Some("2026-08-14T15:04:05Z".into()),
    };
    let value = serde_json::to_value(status).unwrap();
    assert_eq!(value["state"], "connected");
    assert_eq!(value["transport"], "ble");
    assert_eq!(value["deviceName"], "InkDock E213");
    assert_eq!(value["lastAckAt"], "2026-08-14T15:04:05Z");
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
    let directory = std::env::temp_dir().join(format!("tenders-secret-test-{}", Uuid::now_v7()));
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

#[test]
fn numeric_measurement_churn_does_not_create_a_new_material_identity() {
    assert_eq!(
        normalize_numeric_measurements("Rain 61% in 84 min · gust 42.5 mph"),
        "Rain #% in # min · gust # mph"
    );
    assert_ne!(
        normalize_numeric_measurements("AAPL +5.2%"),
        normalize_numeric_measurements("AAPL -5.2%")
    );
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
    official.signal = Some(bridgestatus_runtime::ChannelSignalDto {
        headline: "Flash Flood Warning".into(),
        detail: "Life-threatening flash flooding is occurring in downtown Miami.".into(),
        action: "Move to higher ground now.".into(),
        severity: Some("Extreme".into()),
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
    official.signal = Some(bridgestatus_runtime::ChannelSignalDto {
        headline: "Tornado Warning".into(),
        detail: "A tornado warning is active.".into(),
        action: "Take shelter now.".into(),
        severity: Some("Severe".into()),
        expires_at: None,
    });

    assert!(
        quiet_hours_block(&preferences, &snapshot.channels[official_index], &snapshot).unwrap(),
        "a preset name must not promote Severe to the Extreme bypass"
    );
    snapshot.channels[official_index]
        .signal
        .as_mut()
        .unwrap()
        .severity = Some("Extreme".into());
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    replacement.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::NotRecorded;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    let rows = store.list_outbox_history(10).await.unwrap();
    assert_eq!(rows.len(), 2, "the new recipient gets an initial warning");
    let newest: DeliveryRequest = serde_json::from_str(&rows[0].request_json).unwrap();
    assert_eq!(newest.destination.address, "+13055550999");
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
    preferences.whatsapp.consent = bridgestatus_runtime::WhatsAppRecipientConsent::OptedIn;
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
    let mut preferences = engine.get_preferences().await;
    preferences.display.return_home_after = 2;
    let mut snapshot = engine.get_snapshot().await.unwrap();
    for channel in &mut snapshot.channels {
        channel.enabled = matches!(
            channel.id.as_str(),
            "bridge.brickell" | "weather.miami" | "news.local"
        );
        channel.active = false;
        channel.destinations = vec![DestinationIdDto::Epaper];
        channel.presence = if channel.id == "bridge.brickell" {
            SurfacePresence::Home
        } else {
            SurfacePresence::Rotation
        };
    }
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 0).map(|channel| channel.id.as_str()),
        Some("bridge.brickell")
    );
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 1).map(|channel| channel.id.as_str()),
        Some("weather.miami")
    );
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 2).map(|channel| channel.id.as_str()),
        Some("weather.miami")
    );
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 3).map(|channel| channel.id.as_str()),
        Some("bridge.brickell")
    );
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 4).map(|channel| channel.id.as_str()),
        Some("weather.miami")
    );
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
    assert!(select_rotation_channel(&snapshot, &preferences, 0).is_none());
    let active_only = snapshot
        .channels
        .iter_mut()
        .find(|channel| channel.presence == SurfacePresence::ActiveOnly)
        .unwrap();
    active_only.active = true;
    let expected = active_only.id.clone();
    assert_eq!(
        select_rotation_channel(&snapshot, &preferences, 0).map(|channel| &channel.id),
        Some(&expected)
    );
}
