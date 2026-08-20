fn whatsapp_route_changed_or_revoked(old: &AppPreferences, current: &AppPreferences) -> bool {
    old.whatsapp.enabled
        && (!current.whatsapp.enabled
            || !whatsapp_consent_is_current(&current.whatsapp)
            || old.whatsapp.recipient.trim() != current.whatsapp.recipient.trim()
            || old.whatsapp.phone_number_id != current.whatsapp.phone_number_id
            || old.whatsapp.template_name != current.whatsapp.template_name
            || old.whatsapp.language_code != current.whatsapp.language_code
            || old.whatsapp.graph_version != current.whatsapp.graph_version)
}
fn whatsapp_route_fingerprint(preferences: &AppPreferences) -> Option<String> {
    if !preferences.whatsapp.enabled
        || !preferences.whatsapp.token_configured
        || !whatsapp_consent_is_current(&preferences.whatsapp)
        || preferences.whatsapp.recipient.trim().is_empty()
    {
        return None;
    }
    let serialized = serde_json::to_vec(&serde_json::json!({
        "recipient": preferences.whatsapp.recipient.trim(),
        "consentRecipient": preferences.whatsapp.consent_recipient,
        "consentRecordedAtMillis": preferences.whatsapp.consent_recorded_at_millis,
        "phoneNumberId": preferences.whatsapp.phone_number_id,
        "templateName": preferences.whatsapp.template_name,
        "languageCode": preferences.whatsapp.language_code,
        "graphVersion": preferences.whatsapp.graph_version,
    }))
    .ok()?;
    Some(format!("{:x}", Sha256::digest(serialized)))
}

async fn suppress_and_reset_whatsapp_route(
    store: &Store,
    now: &str,
    reason: &str,
) -> Result<(), String> {
    store
        .suppress_route_and_set_json(
            WHATSAPP_ROUTE_ID,
            now,
            reason,
            DISPATCH_TRACKER_KEY,
            &WhatsAppDispatchTracker::default(),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn test_whatsapp(state: State<'_, DesktopState>) -> Result<MutationResult, String> {
    // Consent, recipient, token, and route identity must remain fixed for the
    // complete send. Preference/token mutations take this same lock.
    let _dispatch_guard = state.dispatch_lock.lock().await;
    let preferences = state.engine.get_preferences().await;
    if !preferences.whatsapp.enabled {
        return Ok(mutation_error(
            "Enable WhatsApp delivery and save before sending a test.",
        ));
    }
    if !whatsapp_consent_is_current(&preferences.whatsapp) {
        return Ok(mutation_error(
            "WhatsApp test blocked: opt-in is not bound to the saved recipient.",
        ));
    }
    let token = match state.secret_store.whatsapp_token().await {
        Ok(Some(token)) => token,
        Ok(None) => {
            return Ok(mutation_error(
                "No Meta access token is stored locally.",
            ));
        }
        Err(error) => return Ok(mutation_error(error)),
    };
    let snapshot = match state.engine.get_snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Ok(mutation_error(format!(
                "Could not build the test notice: {error}"
            )));
        }
    };
    let secret = match SecretValue::new(token) {
        Ok(secret) => secret,
        Err(error) => {
            return Ok(mutation_error(format!(
                "Stored Meta token is invalid: {error}"
            )));
        }
    };
    let config = WhatsAppConfig::cloud(
        &preferences.whatsapp.graph_version,
        &preferences.whatsapp.phone_number_id,
        &preferences.whatsapp.template_name,
        &preferences.whatsapp.language_code,
        TokenSource::Inline(secret),
    );
    let adapter = WhatsAppCloud::new(
        config,
        Arc::new(EnvironmentSecretResolver),
        Arc::new(ReqwestExecutor::default()),
    );
    let request = delivery_test_request(&preferences, &snapshot);
    Ok(match adapter.deliver(&request).await {
        Ok(receipt) => MutationResult {
            ok: true,
            message: format!(
                "Meta accepted the template test{}; delivery awaits a signed status receipt.",
                receipt
                    .provider_message_id
                    .as_deref()
                    .map(|id| format!(" ({})", safe_id(id)))
                    .unwrap_or_default()
            ),
        },
        Err(error) => mutation_error(error.to_string()),
    })
}

fn delivery_test_request(preferences: &AppPreferences, _snapshot: &AppSnapshot) -> DeliveryRequest {
    let now_ms = Timestamp::now().as_millisecond();
    DeliveryRequest {
        outbox_id: Uuid::now_v7(),
        incident_id: Uuid::now_v7(),
        material_revision: 1,
        deduplication_key: format!("desktop-test-{now_ms}"),
        reason: DeliveryReason::Test,
        destination: Destination {
            id: "desktop.whatsapp.test".into(),
            address: preferences.whatsapp.recipient.clone(),
            locale: Some(preferences.whatsapp.language_code.clone()),
            messaging_consent: MessagingConsent::OptedIn {
                recorded_at_millis: preferences
                    .whatsapp
                    .consent_recorded_at_millis
                    .unwrap_or_default(),
            },
        },
        notice: Notice {
            subject: "TEST ONLY — BrickellStatus delivery check".into(),
            state: NoticeState::Unknown,
            road_meaning:
                "This verifies the configured WhatsApp route. It does not report a live bridge or weather condition."
                    .into(),
            action: "No action required. Confirm only that this test message arrived.".into(),
            eta: None,
            confidence_percent: None,
            evidence: vec!["TEST MESSAGE · LIVE ALERT STATE OMITTED".into()],
            source_label: "BrickellStatus route test".into(),
            source_age_seconds: 0,
        },
        created_at_millis: now_ms,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WhatsAppDispatchTracker {
    /// Binds dispatch state to the exact recipient, consent record, sender,
    /// template, locale, and Graph version that observed it.
    route_fingerprint: Option<String>,
    channels: BTreeMap<String, ChannelDispatchState>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDispatchState {
    incident_id: Option<Uuid>,
    revision: u32,
    last_material: Option<String>,
    active: bool,
    announced: bool,
}

async fn enqueue_material_whatsapp_updates(
    store: &Store,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> Result<(), String> {
    let stored = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .map_err(|error| error.to_string())?;
    let mut tracker = stored.clone().unwrap_or_default();
    let now_ms = Timestamp::now().as_millisecond();
    let now = iso_at(now_ms)?;

    let route_fingerprint = whatsapp_route_fingerprint(preferences);
    let Some(route_fingerprint) = route_fingerprint else {
        if stored.is_some() && (!tracker.channels.is_empty() || tracker.route_fingerprint.is_some())
        {
            tracker.channels.clear();
            tracker.route_fingerprint = None;
            store
                .set_json(DISPATCH_TRACKER_KEY, &tracker, &now)
                .await
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    };

    let mut changed = false;
    if tracker.route_fingerprint.as_deref() != Some(route_fingerprint.as_str()) {
        // Never let a new recipient inherit "already announced" state or an
        // all-clear from a previous route. This also repairs a preference save
        // whose best-effort route cleanup was interrupted after persistence.
        tracker.channels.clear();
        tracker.route_fingerprint = Some(route_fingerprint);
        changed = true;
    }

    for channel in &snapshot.channels {
        let mut previous = tracker
            .channels
            .get(&channel.id)
            .cloned()
            .unwrap_or_default();
        if !whatsapp_route_configured(channel) {
            if previous != ChannelDispatchState::default() {
                tracker
                    .channels
                    .insert(channel.id.clone(), ChannelDispatchState::default());
                changed = true;
            }
            continue;
        }
        if previous.active
            && !previous.announced
            && let Some(incident_id) = previous.incident_id
            && store
                .outbox_revision_was_accepted(
                    WHATSAPP_ROUTE_ID,
                    incident_id,
                    i64::from(previous.revision),
                )
                .await
                .map_err(|error| error.to_string())?
        {
            // Recover the narrow crash boundary between Meta accepting
            // a warning and an older build recording that acceptance
            // in the dispatch tracker. Only an accepted/delivered row
            // for this exact incident revision can promote it.
            previous.announced = true;
            tracker
                .channels
                .insert(channel.id.clone(), previous.clone());
            changed = true;
        }
        let trustworthy = matches!(
            channel.availability,
            AvailabilityDto::Fresh | AvailabilityDto::Delayed
        );
        let active = trustworthy && interrupt_allows(channel, preferences, snapshot);
        // Partial usable coverage may raise a positive signal, but absence is
        // only authoritative when every configured source is fresh. Otherwise
        // one healthy sibling could falsely clear an alert-bearing source that
        // merely went offline.
        let resolution_trustworthy = trustworthy && channel.coverage_complete;
        let became_inactive = previous.active && !active && resolution_trustworthy;
        let resolved = became_inactive && previous.announced;
        if became_inactive && !previous.announced {
            tracker.channels.insert(
                channel.id.clone(),
                ChannelDispatchState {
                    incident_id: previous.incident_id,
                    revision: previous.revision,
                    last_material: Some("resolved".into()),
                    active: false,
                    announced: false,
                },
            );
            changed = true;
            continue;
        }
        if !active && !resolved {
            continue;
        }
        if quiet_hours_block(preferences, channel, snapshot)? {
            // Hold both warnings and all-clears. Keeping an announced prior
            // incident active allows one still-current resolution to emit
            // after quiet hours instead of silently dropping it.
            continue;
        }

        let material = if active {
            material_channel_state(channel, snapshot)
        } else {
            "resolved".into()
        };
        if previous.last_material.as_deref() == Some(material.as_str()) {
            continue;
        }
        let incident_id = if active && !previous.active {
            Uuid::now_v7()
        } else {
            previous.incident_id.unwrap_or_else(Uuid::now_v7)
        };
        let revision = previous.revision.saturating_add(1).max(1);
        let outbox_id = Uuid::now_v7();
        let request = delivery_request_for_channel(
            preferences,
            snapshot,
            channel,
            active,
            incident_id,
            outbox_id,
            revision,
            now_ms,
            &material,
        );
        let action = if active {
            "material_update"
        } else {
            "resolved"
        };
        tracker.channels.insert(
            channel.id.clone(),
            if active {
                ChannelDispatchState {
                    incident_id: Some(incident_id),
                    revision,
                    last_material: Some(material),
                    active: true,
                    announced: previous.active && previous.announced,
                }
            } else {
                ChannelDispatchState {
                    incident_id: Some(incident_id),
                    revision,
                    last_material: Some(material),
                    active: false,
                    announced: previous.announced,
                }
            },
        );
        store
            .commit_delivery_transition(
                &IncidentRecord {
                    id: incident_id,
                    channel_id: &channel.id,
                    state: if active { "active" } else { "resolved" },
                    urgency: urgency_key(channel.priority.urgency),
                    material_revision: i64::from(revision),
                    fingerprint: tracker
                        .channels
                        .get(&channel.id)
                        .and_then(|state| state.last_material.as_deref())
                        .unwrap_or("unknown"),
                    // Incident history needs the sourced notice, never the
                    // recipient address carried by the outbound envelope.
                    payload: &request.notice,
                    opened_at: &now,
                    updated_at: &now,
                    resolved_at: (!active).then_some(now.as_str()),
                },
                &OutboxRecord {
                    id: outbox_id,
                    route_id: WHATSAPP_ROUTE_ID,
                    incident_id,
                    material_revision: i64::from(revision),
                    action,
                    request: &request,
                    next_attempt_at: &now,
                    created_at: &now,
                },
                DISPATCH_TRACKER_KEY,
                &tracker,
                &now,
            )
            .await
            .map_err(|error| error.to_string())?;
        changed = true;
    }
    if changed || stored.is_none() {
        store
            .set_json(DISPATCH_TRACKER_KEY, &tracker, &now)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn dispatch_desktop_notifications(
    app: &AppHandle,
    store: &Store,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> Result<(), String> {
    let stored = store
        .get_json::<WhatsAppDispatchTracker>(NOTIFICATION_TRACKER_KEY)
        .await
        .map_err(|error| error.to_string())?;
    let mut tracker = stored.clone().unwrap_or_default();
    let mut changed = false;
    let mut first_error = None;
    for channel in &snapshot.channels {
        let previous = tracker
            .channels
            .get(&channel.id)
            .cloned()
            .unwrap_or_default();
        if !desktop_route_configured(channel) {
            if previous != ChannelDispatchState::default() {
                tracker
                    .channels
                    .insert(channel.id.clone(), ChannelDispatchState::default());
                changed = true;
            }
            continue;
        }
        let trustworthy = matches!(
            channel.availability,
            AvailabilityDto::Fresh | AvailabilityDto::Delayed
        );
        let active = trustworthy && interrupt_allows(channel, preferences, snapshot);
        let resolution_trustworthy = trustworthy && channel.coverage_complete;
        let became_inactive = previous.active && !active && resolution_trustworthy;
        let resolved = became_inactive && previous.announced;
        if became_inactive && !previous.announced {
            tracker.channels.insert(
                channel.id.clone(),
                ChannelDispatchState {
                    incident_id: previous.incident_id,
                    revision: previous.revision,
                    last_material: Some("resolved".into()),
                    active: false,
                    announced: false,
                },
            );
            changed = true;
            continue;
        }
        if !active && !resolved {
            continue;
        }
        if quiet_hours_block(preferences, channel, snapshot)? {
            continue;
        }
        let material = if active {
            material_channel_state(channel, snapshot)
        } else {
            "resolved".into()
        };
        if previous.last_material.as_deref() == Some(material.as_str()) {
            continue;
        }
        let (title, body) = desktop_notification_copy(channel, active);
        if let Err(error) = app
            .notification()
            .builder()
            .title(bounded_text(&title, 96))
            .body(bounded_text(&body, 320))
            .show()
        {
            first_error
                .get_or_insert_with(|| format!("Native notification was not accepted: {error}"));
            continue;
        }
        tracker.channels.insert(
            channel.id.clone(),
            ChannelDispatchState {
                incident_id: None,
                revision: previous.revision.saturating_add(1),
                last_material: Some(material),
                active,
                // The pinned desktop notification plugin only confirms that
                // it spawned an OS task, not that the OS displayed it. Until
                // an awaited host receipt exists, never infer an all-clear.
                announced: false,
            },
        );
        changed = true;
    }
    if changed || stored.is_none() {
        let now = Timestamp::now().to_string();
        store
            .set_json(NOTIFICATION_TRACKER_KEY, &tracker, &now)
            .await
            .map_err(|error| error.to_string())?;
    }
    first_error.map_or(Ok(()), Err)
}

async fn process_whatsapp_outbox(
    store: &Store,
    secret_store: &LocalSecretStore,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> Result<(), String> {
    let now_ms = Timestamp::now().as_millisecond();
    let now = iso_at(now_ms)?;
    let lease_until = iso_at(now_ms.saturating_add(60_000))?;
    let Some(lease) = store
        .lease_next(&now, &lease_until)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if lease.route_id != WHATSAPP_ROUTE_ID {
        suppress_leased_outbox(
            store,
            &lease,
            &now,
            "This desktop worker does not own the stored delivery route",
        )
        .await?;
        return Ok(());
    }
    let request = match serde_json::from_str::<DeliveryRequest>(&lease.request_json) {
        Ok(request) => request,
        Err(error) => {
            suppress_leased_outbox(
                store,
                &lease,
                &now,
                &format!("Invalid stored request: {error}"),
            )
            .await?;
            return Ok(());
        }
    };
    let Some(channel) = snapshot
        .channels
        .iter()
        .find(|channel| channel.id == request.destination.id)
    else {
        suppress_leased_outbox(
            store,
            &lease,
            &now,
            "The originating channel no longer exists",
        )
        .await?;
        return Ok(());
    };
    let route_is_live = preferences.whatsapp.enabled
        && preferences.whatsapp.token_configured
        && whatsapp_consent_is_current(&preferences.whatsapp)
        && request.destination.address.trim() == preferences.whatsapp.recipient.trim()
        && whatsapp_route_configured(channel);
    if !route_is_live {
        suppress_leased_outbox(
            store,
            &lease,
            &now,
            "Route disabled, token removed, recipient changed, or opt-in is no longer current",
        )
        .await?;
        return Ok(());
    }
    let tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    if let Err(reason) = validate_current_outbox(
        &lease,
        &request,
        &tracker,
        channel,
        preferences,
        snapshot,
        now_ms,
    ) {
        if reason.starts_with("Notice exceeded this channel's")
            || reason == "The channel state changed before this notice could be delivered"
            || reason == "Complete usable source coverage is required before sending an all-clear"
        {
            suppress_and_rearm_current_outbox(store, &lease, &request, &tracker, &now, &reason)
                .await?;
        } else {
            suppress_leased_outbox(store, &lease, &now, &reason).await?;
        }
        return Ok(());
    }
    if quiet_hours_block(preferences, channel, snapshot)? {
        let retry_at = iso_at(now_ms.saturating_add(5 * 60_000))?;
        store
            .mark_outbox(
                &lease.id,
                "failed",
                &now,
                None,
                Some("Held by configured quiet hours"),
                Some(&retry_at),
            )
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let token = match secret_store.whatsapp_token().await {
        Ok(Some(token)) => token,
        Ok(None) => {
            defer_outbox(
                store,
                &lease.id,
                &now,
                now_ms,
                "Meta token is not available in local credential storage",
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            defer_outbox(store, &lease.id, &now, now_ms, &error).await?;
            return Ok(());
        }
    };
    let adapter = match whatsapp_adapter(preferences, token) {
        Ok(adapter) => adapter,
        Err(error) => {
            store
                .mark_outbox(
                    &lease.id,
                    "failed",
                    &now,
                    None,
                    Some(&error),
                    Some("9999-12-31T23:59:59Z"),
                )
                .await
                .map_err(|storage| storage.to_string())?;
            return Ok(());
        }
    };
    match adapter.deliver(&request).await {
        Ok(receipt) => {
            mark_whatsapp_accepted(
                store,
                &lease,
                &request,
                &now,
                receipt.provider_message_id.as_deref(),
            )
            .await?;
        }
        Err(error) => {
            let (status, next_attempt_at) = if error.kind == DeliveryFailureKind::Suppressed {
                ("suppressed", None)
            } else if error.retryable() {
                let exponent = u32::try_from(lease.attempts.max(0))
                    .unwrap_or(u32::MAX)
                    .min(5);
                let local_backoff = 30_i64.saturating_mul(1_i64 << exponent).min(15 * 60);
                let delay_seconds = error
                    .retry_after_seconds
                    .and_then(|seconds| i64::try_from(seconds).ok())
                    .unwrap_or(local_backoff)
                    .clamp(1, 24 * 60 * 60);
                (
                    "failed",
                    Some(iso_at(now_ms.saturating_add(delay_seconds * 1_000))?),
                )
            } else {
                ("failed", Some("9999-12-31T23:59:59Z".into()))
            };
            store
                .mark_outbox(
                    &lease.id,
                    status,
                    &now,
                    None,
                    Some(&error.to_string()),
                    next_attempt_at.as_deref(),
                )
                .await
                .map_err(|storage| storage.to_string())?;
        }
    }
    Ok(())
}

async fn mark_whatsapp_accepted(
    store: &Store,
    lease: &OutboxLease,
    request: &DeliveryRequest,
    now: &str,
    provider_message_id: Option<&str>,
) -> Result<(), String> {
    let mut tracker = store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    if let Some(current) = tracker.channels.get_mut(&request.destination.id)
        && current.active
        && current.incident_id == Some(request.incident_id)
        && current.revision == request.material_revision
    {
        current.announced = true;
    }
    store
        .mark_outbox_and_set_json(
            &lease.id,
            "accepted",
            now,
            provider_message_id,
            None,
            None,
            DISPATCH_TRACKER_KEY,
            &tracker,
        )
        .await
        .map_err(|error| error.to_string())
}

async fn suppress_leased_outbox(
    store: &Store,
    lease: &OutboxLease,
    now: &str,
    reason: &str,
) -> Result<(), String> {
    store
        .mark_outbox(&lease.id, "suppressed", now, None, Some(reason), None)
        .await
        .map_err(|error| error.to_string())
}

async fn suppress_and_rearm_current_outbox(
    store: &Store,
    lease: &OutboxLease,
    request: &DeliveryRequest,
    tracker: &WhatsAppDispatchTracker,
    now: &str,
    reason: &str,
) -> Result<(), String> {
    let mut rearmed = tracker.clone();
    let Some(current) = rearmed.channels.get_mut(&request.destination.id) else {
        return suppress_leased_outbox(store, lease, now, reason).await;
    };
    if current.incident_id != Some(request.incident_id)
        || current.revision != request.material_revision
    {
        return suppress_leased_outbox(store, lease, now, reason).await;
    }

    // The current representation aged out or became temporarily unverifiable
    // while queued. Preserve incident/acceptance history, but remove the
    // material edge so one fresh representation can be created if the same
    // signal becomes trustworthy again. A queued all-clear is modeled as
    // active solely to recreate its transition; it is never sent unless
    // complete coverage still proves the channel inactive on that next cycle.
    current.last_material = None;
    if !current.active && current.announced {
        current.active = true;
    }
    store
        .mark_outbox_and_set_json(
            &lease.id,
            "suppressed",
            now,
            None,
            Some(reason),
            None,
            DISPATCH_TRACKER_KEY,
            &rearmed,
        )
        .await
        .map_err(|error| error.to_string())
}

fn validate_current_outbox(
    lease: &OutboxLease,
    request: &DeliveryRequest,
    tracker: &WhatsAppDispatchTracker,
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
    now_ms: i64,
) -> Result<(), String> {
    if lease.id != request.outbox_id.to_string()
        || lease.incident_id != request.incident_id.to_string()
        || lease.material_revision != i64::from(request.material_revision)
    {
        return Err("Stored outbox identity does not match its request envelope".into());
    }
    let Some(current) = tracker.channels.get(&channel.id) else {
        return Err("The material transition is no longer current".into());
    };
    if current.incident_id != Some(request.incident_id)
        || current.revision != request.material_revision
        || i64::from(current.revision) != lease.material_revision
    {
        return Err("A newer material revision superseded this notice".into());
    }
    if !matches!(
        request.destination.messaging_consent,
        MessagingConsent::OptedIn { recorded_at_millis }
            if Some(recorded_at_millis) == preferences.whatsapp.consent_recorded_at_millis
    ) {
        return Err("The stored notice is not bound to the current consent record".into());
    }

    let trustworthy = matches!(
        channel.availability,
        AvailabilityDto::Fresh | AvailabilityDto::Delayed
    );
    let current_active = trustworthy && interrupt_allows(channel, preferences, snapshot);
    if !current.active && (!trustworthy || !channel.coverage_complete) {
        return Err(
            "Complete usable source coverage is required before sending an all-clear".into(),
        );
    }
    let expected_action = if current.active {
        "material_update"
    } else {
        "resolved"
    };
    if lease.action != expected_action || current.active != current_active {
        return Err("The channel state changed before this notice could be delivered".into());
    }
    let expected_material = if current.active {
        material_channel_state(channel, snapshot)
    } else {
        "resolved".into()
    };
    if current.last_material.as_deref() != Some(expected_material.as_str())
        || request.deduplication_key != format!("{}:{expected_material}", channel.id)
    {
        return Err("The notice content has been superseded by a newer channel state".into());
    }

    let Some(preference) = preferences
        .profile
        .channels
        .iter()
        .find(|preference| preference.id == channel.id)
    else {
        return Err("The originating channel no longer has a saved policy".into());
    };
    if request.created_at_millis <= 0
        || request.created_at_millis > now_ms.saturating_add(5 * 60_000)
    {
        return Err("The notice creation time is invalid".into());
    }
    let max_age_ms = i64::from(preference.max_age_minutes).saturating_mul(60_000);
    if now_ms.saturating_sub(request.created_at_millis) > max_age_ms {
        return Err(format!(
            "Notice exceeded this channel's {} minute freshness limit",
            preference.max_age_minutes
        ));
    }
    Ok(())
}

async fn defer_outbox(
    store: &Store,
    id: &str,
    now: &str,
    now_ms: i64,
    error: &str,
) -> Result<(), String> {
    let retry_at = iso_at(now_ms.saturating_add(5 * 60_000))?;
    store
        .mark_outbox(id, "failed", now, None, Some(error), Some(&retry_at))
        .await
        .map_err(|storage| storage.to_string())
}

fn whatsapp_adapter(preferences: &AppPreferences, token: String) -> Result<WhatsAppCloud, String> {
    let token = SecretValue::new(token).map_err(|error| error.to_string())?;
    let config = WhatsAppConfig::cloud(
        &preferences.whatsapp.graph_version,
        &preferences.whatsapp.phone_number_id,
        &preferences.whatsapp.template_name,
        &preferences.whatsapp.language_code,
        TokenSource::Inline(token),
    );
    Ok(WhatsAppCloud::new(
        config,
        Arc::new(EnvironmentSecretResolver),
        Arc::new(ReqwestExecutor::default()),
    ))
}

#[allow(clippy::too_many_arguments)]
fn delivery_request_for_channel(
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
    channel: &ChannelSnapshot,
    active: bool,
    incident_id: Uuid,
    outbox_id: Uuid,
    revision: u32,
    now_ms: i64,
    material: &str,
) -> DeliveryRequest {
    let is_bridge = channel.kind == ChannelKindDto::Bridge;
    let state = if is_bridge {
        match snapshot.decision.state {
            BridgeStateDto::Clear => NoticeState::Clear,
            BridgeStateDto::Likely => NoticeState::Likely,
            BridgeStateDto::Open => NoticeState::Open,
        }
    } else if active {
        NoticeState::Alert
    } else {
        NoticeState::Resolved
    };
    let eta = is_bridge
        .then(|| {
            snapshot.decision.eta_min.map(|minimum| {
                DeliveryEtaRange::new(minimum, snapshot.decision.eta_max.unwrap_or(minimum))
            })
        })
        .flatten();
    let confidence_percent = state
        .is_predictive()
        .then(|| snapshot.decision.confidence_bps.map(bps_to_percent))
        .flatten();
    let signal = active.then_some(channel.signal.as_ref()).flatten();
    let subject = if is_bridge || !active {
        channel.title.clone()
    } else {
        signal.map_or_else(|| channel.title.clone(), |signal| signal.headline.clone())
    };
    let road_meaning = if is_bridge {
        snapshot.decision.meaning.clone()
    } else if active {
        signal.map_or_else(
            || nonempty_or(&channel.summary, "An active signal was reported."),
            |signal| nonempty_or(&signal.detail, "An active signal was reported."),
        )
    } else {
        format!("{} is no longer reporting an active alert.", channel.title)
    };
    let action = if is_bridge {
        snapshot.decision.action.clone()
    } else if active {
        signal.map_or_else(
            || "Review the sourced signal before changing plans.".into(),
            |signal| signal.action.clone(),
        )
    } else {
        "No action is required unless conditions change again.".into()
    };
    DeliveryRequest {
        outbox_id,
        incident_id,
        material_revision: revision,
        deduplication_key: format!("{}:{material}", channel.id),
        reason: DeliveryReason::StateTransition,
        destination: Destination {
            id: channel.id.clone(),
            address: preferences.whatsapp.recipient.clone(),
            locale: Some(preferences.whatsapp.language_code.clone()),
            messaging_consent: MessagingConsent::OptedIn {
                recorded_at_millis: preferences
                    .whatsapp
                    .consent_recorded_at_millis
                    .unwrap_or_default(),
            },
        },
        notice: Notice {
            subject,
            state,
            road_meaning,
            action,
            eta,
            confidence_percent,
            evidence: if active {
                let mut evidence = Vec::new();
                if !channel.summary.trim().is_empty() {
                    evidence.push(channel.summary.clone());
                }
                if let Some(severity) = signal.and_then(|signal| signal.severity.as_deref()) {
                    evidence.push(format!("Severity: {severity}"));
                }
                if let Some(expires_at) = signal.and_then(|signal| signal.expires_at.as_deref()) {
                    evidence.push(format!("Expires: {expires_at}"));
                }
                evidence
            } else {
                Vec::new()
            },
            source_label: nonempty_or(&channel.source_label, "BrickellStatus"),
            source_age_seconds: channel.age_seconds,
        },
        created_at_millis: now_ms,
    }
}

fn material_channel_state(channel: &ChannelSnapshot, snapshot: &AppSnapshot) -> String {
    if channel.kind == ChannelKindDto::Bridge {
        let eta_band = snapshot
            .decision
            .eta_min
            .map(|minutes| match minutes {
                0..=3 => "0-3",
                4..=6 => "4-6",
                7..=10 => "7-10",
                11..=15 => "11-15",
                _ => "16+",
            })
            .unwrap_or("none");
        let confidence_band = snapshot
            .decision
            .confidence_bps
            .map(|bps| match bps {
                0..=4_999 => "low",
                5_000..=7_499 => "moderate",
                7_500..=8_999 => "high",
                _ => "very-high",
            })
            .unwrap_or("confirmed");
        format!("{:?}:{eta_band}:{confidence_band}", snapshot.decision.state)
    } else {
        match channel.kind {
            ChannelKindDto::Official
            | ChannelKindDto::Hurricane
            | ChannelKindDto::News
            | ChannelKindDto::Sports
            | ChannelKindDto::Earthquake => {
                format!("active:{}", delivered_signal_material(channel))
            }
            // Banded identity: crossing a band is news, drifting inside one is
            // not. The band comes from the runtime, where the numbers are still
            // typed, rather than being recovered from prose here.
            ChannelKindDto::Weather | ChannelKindDto::Markets => channel
                .signal
                .as_ref()
                .and_then(|signal| signal.band.as_deref())
                .map_or_else(
                    || format!("active:{}", normalize_material_text(&channel.summary)),
                    |band| format!("active:{band}"),
                ),
            ChannelKindDto::System | ChannelKindDto::Bridge => {
                format!("active:{}", normalize_material_text(&channel.summary))
            }
        }
    }
}

fn delivered_signal_material(channel: &ChannelSnapshot) -> String {
    let serialized = serde_json::to_vec(&serde_json::json!({
        "signal": channel.signal,
        "summary": channel.summary,
        "sourceLabel": channel.source_label,
    }))
    .unwrap_or_else(|_| channel.material_key.as_bytes().to_vec());
    format!("{:x}", Sha256::digest(serialized))
}

fn normalize_material_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512)
        .collect()
}

fn nonempty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.into()
    } else {
        value.into()
    }
}

fn whatsapp_route_configured(channel: &ChannelSnapshot) -> bool {
    channel.enabled
        && !matches!(
            channel.interrupt_preset,
            InterruptPreset::Off | InterruptPreset::Custom
        )
        && channel.destinations.contains(&DestinationIdDto::Whatsapp)
}

fn urgency_key(urgency: UrgencyDto) -> &'static str {
    match urgency {
        UrgencyDto::Routine => "routine",
        UrgencyDto::HeadsUp => "heads_up",
        UrgencyDto::Action => "action",
        UrgencyDto::Emergency => "emergency",
    }
}


fn desktop_notification_copy(channel: &ChannelSnapshot, active: bool) -> (String, String) {
    if !active {
        return (
            format!("{} · Resolved", channel.title),
            format!(
                "The prior active signal has cleared. Source: {}",
                channel.source_label
            ),
        );
    }
    let Some(signal) = channel.signal.as_ref() else {
        return (
            format!("{} · Alert", channel.title),
            format!("{} · Source: {}", channel.summary, channel.source_label),
        );
    };
    let title = signal.severity.as_deref().map_or_else(
        || signal.headline.clone(),
        |severity| format!("{} · {severity}", signal.headline),
    );
    let mut facts = vec![signal.detail.clone(), signal.action.clone()];
    if let Some(expires_at) = signal.expires_at.as_deref() {
        facts.push(format!("Expires {expires_at}"));
    }
    facts.push(format!("Source: {}", channel.source_label));
    (title, facts.join(" · "))
}

fn desktop_route_configured(channel: &ChannelSnapshot) -> bool {
    channel.enabled
        && !matches!(
            channel.interrupt_preset,
            InterruptPreset::Off | InterruptPreset::Custom
        )
        && channel.destinations.contains(&DestinationIdDto::Desktop)
}

/// Applies the plain-language interrupt presets to the current event, rather
/// than treating every non-off value as equivalent.
fn quiet_hours_block(
    preferences: &AppPreferences,
    channel: &ChannelSnapshot,
    snapshot: &AppSnapshot,
) -> Result<bool, String> {
    let quiet = &preferences.profile.quiet_hours;
    if !quiet.enabled {
        return Ok(false);
    }
    let start = parse_clock_minutes(&quiet.start)?;
    let end = parse_clock_minutes(&quiet.end)?;
    let time_zone = TimeZone::get(&quiet.time_zone)
        .map_err(|error| format!("Quiet-hours time zone is invalid: {error}"))?;
    let evaluated_at = snapshot
        .generated_at
        .parse::<Timestamp>()
        .map_err(|error| format!("Snapshot timestamp is invalid: {error}"))?;
    let local = evaluated_at.to_zoned(time_zone);
    let hour = u16::try_from(local.hour())
        .map_err(|_| "Quiet-hours local hour was negative.".to_owned())?;
    let minute = u16::try_from(local.minute())
        .map_err(|_| "Quiet-hours local minute was negative.".to_owned())?;
    let current = hour * 60 + minute;
    let in_quiet_hours = if start == end {
        true
    } else if start < end {
        (start..end).contains(&current)
    } else {
        current >= start || current < end
    };
    if !in_quiet_hours {
        return Ok(false);
    }
    // What counts as an emergency is decided once, in the engine, and carried on
    // the snapshot. This used to re-derive it from two hardcoded cases -- an
    // extreme official alert, or an open bridge -- which meant the urgency the
    // engine computed was ignored here and, for instance, a magnitude 7
    // earthquake was held until morning despite already being classified
    // Emergency.
    Ok(!(quiet.bypass_emergency && channel.priority.urgency == UrgencyDto::Emergency))
}

fn parse_clock_minutes(value: &str) -> Result<u16, String> {
    let (hour, minute) = value
        .split_once(':')
        .ok_or_else(|| format!("Invalid quiet-hours time {value:?}"))?;
    let hour = hour
        .parse::<u16>()
        .map_err(|_| format!("Invalid quiet-hours hour {hour:?}"))?;
    let minute = minute
        .parse::<u16>()
        .map_err(|_| format!("Invalid quiet-hours minute {minute:?}"))?;
    if hour > 23 || minute > 59 {
        return Err(format!("Invalid quiet-hours time {value:?}"));
    }
    Ok(hour * 60 + minute)
}

fn iso_at(milliseconds: i64) -> Result<String, String> {
    Timestamp::from_millisecond(milliseconds)
        .map(|timestamp| timestamp.to_string())
        .map_err(|error| format!("Invalid dispatch timestamp: {error}"))
}
