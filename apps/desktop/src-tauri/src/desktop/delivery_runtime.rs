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
            return Ok(mutation_error("No Meta access token is stored locally."));
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WhatsAppDispatchTracker {
    /// Binds dispatch state to the exact recipient, consent record, sender,
    /// template, locale, and Graph version that observed it.
    route_fingerprint: Option<String>,
    channels: BTreeMap<String, ChannelDispatchState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDispatchState {
    incident_id: Option<Uuid>,
    revision: u32,
    last_material: Option<String>,
    active: bool,
    announced: bool,
    /// Retained so an item-level all-clear can name the item that disappeared.
    /// Older tracker JSON has neither field and continues to deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signal: Option<brickellstatus_runtime::ChannelSignalDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    priority: Option<brickellstatus_runtime::ChannelPriorityDto>,
}

const NOTICE_TRACKER_SEPARATOR: &str = "::notice::";

#[derive(Clone, Debug)]
struct ChannelDispatchTarget {
    tracker_key: String,
    signal: Option<brickellstatus_runtime::ChannelSignalDto>,
    priority: brickellstatus_runtime::ChannelPriorityDto,
    item_level: bool,
}

fn notice_tracker_key(channel_id: &str, notice_key: &str) -> String {
    format!("{channel_id}{NOTICE_TRACKER_SEPARATOR}{notice_key}")
}

fn tracker_key_belongs_to_channel(key: &str, channel_id: &str) -> bool {
    key == channel_id
        || key
            .strip_prefix(channel_id)
            .is_some_and(|suffix| suffix.starts_with(NOTICE_TRACKER_SEPARATOR))
}

fn channel_dispatch_view(
    channel: &ChannelSnapshot,
    target: &ChannelDispatchTarget,
    active: bool,
) -> ChannelSnapshot {
    let mut view = channel.clone();
    view.active = active;
    view.signal.clone_from(&target.signal);
    view.priority = target.priority;
    view.notices.clear();
    view
}

fn current_channel_dispatch_targets(
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> Vec<ChannelDispatchTarget> {
    if !channel.enabled || !channel.active {
        return Vec::new();
    }

    if channel.notices.is_empty() {
        let target = ChannelDispatchTarget {
            tracker_key: channel.id.clone(),
            signal: channel.signal.clone(),
            priority: channel.priority,
            item_level: false,
        };
        return interrupt_allows(
            &channel_dispatch_view(channel, &target, true),
            preferences,
            snapshot,
        )
        .then_some(target)
        .into_iter()
        .collect();
    }

    channel
        .notices
        .iter()
        .map(|notice| ChannelDispatchTarget {
            tracker_key: notice_tracker_key(&channel.id, &notice.key),
            signal: Some(notice.signal.clone()),
            priority: notice.priority,
            item_level: true,
        })
        .filter(|target| {
            interrupt_allows(
                &channel_dispatch_view(channel, target, true),
                preferences,
                snapshot,
            )
        })
        .collect()
}

fn dispatch_material(
    channel: &ChannelSnapshot,
    snapshot: &AppSnapshot,
    target: &ChannelDispatchTarget,
) -> String {
    if !target.item_level {
        return material_channel_state(&channel_dispatch_view(channel, target, true), snapshot);
    }

    let Some(signal) = target.signal.as_ref() else {
        return "active:unknown".into();
    };
    match channel.kind {
        ChannelKindDto::Weather | ChannelKindDto::Markets => signal.band.as_deref().map_or_else(
            || format!("active:{}", normalize_material_text(&signal.detail)),
            |band| format!("active:{band}"),
        ),
        _ => {
            let serialized = serde_json::to_vec(&serde_json::json!({
                "signal": signal,
                "sourceLabel": channel.source_label,
            }))
            .unwrap_or_else(|_| signal.headline.as_bytes().to_vec());
            format!("active:{:x}", Sha256::digest(serialized))
        }
    }
}

fn migrate_legacy_channel_tracker(
    tracker: &mut WhatsAppDispatchTracker,
    channel: &ChannelSnapshot,
    snapshot: &AppSnapshot,
    targets: &[ChannelDispatchTarget],
) -> bool {
    if channel.notices.is_empty() || targets.is_empty() {
        return false;
    }
    let Some(mut legacy) = tracker.channels.remove(&channel.id) else {
        return false;
    };
    if !legacy.active {
        return true;
    }

    let target = &targets[0];
    legacy.last_material = legacy
        .last_material
        .as_ref()
        .map(|_| dispatch_material(channel, snapshot, target));
    legacy.signal.clone_from(&target.signal);
    legacy.priority = Some(target.priority);
    tracker
        .channels
        .entry(target.tracker_key.clone())
        .or_insert(legacy);
    true
}

fn dispatch_state_keys(tracker: &WhatsAppDispatchTracker, channel_id: &str) -> Vec<String> {
    tracker
        .channels
        .keys()
        .filter(|key| tracker_key_belongs_to_channel(key, channel_id))
        .cloned()
        .collect()
}

fn tracker_state_for_request<'a>(
    tracker: &'a WhatsAppDispatchTracker,
    request: &DeliveryRequest,
) -> Option<(&'a str, &'a ChannelDispatchState)> {
    tracker
        .channels
        .iter()
        .find(|(_, state)| {
            state.incident_id == Some(request.incident_id)
                && state.revision == request.material_revision
        })
        .map(|(key, state)| (key.as_str(), state))
}

fn current_target_for_key(
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
    tracker_key: &str,
) -> Option<ChannelDispatchTarget> {
    current_channel_dispatch_targets(channel, preferences, snapshot)
        .into_iter()
        .find(|target| target.tracker_key == tracker_key)
}

fn desktop_notification_material_if_due(
    tracker: &WhatsAppDispatchTracker,
    channel: &ChannelSnapshot,
    snapshot: &AppSnapshot,
    target: &ChannelDispatchTarget,
) -> Option<String> {
    let material = dispatch_material(channel, snapshot, target);
    let unchanged = tracker
        .channels
        .get(&target.tracker_key)
        .is_some_and(|state| {
            state.active && state.last_material.as_deref() == Some(material.as_str())
        });
    (!unchanged).then_some(material)
}

fn record_desktop_notification(
    tracker: &mut WhatsAppDispatchTracker,
    target: &ChannelDispatchTarget,
    material: String,
) {
    let revision = tracker
        .channels
        .get(&target.tracker_key)
        .map_or(1, |state| state.revision.saturating_add(1));
    tracker.channels.insert(
        target.tracker_key.clone(),
        ChannelDispatchState {
            incident_id: None,
            revision,
            last_material: Some(material),
            active: true,
            // The pinned desktop notification plugin only confirms that it
            // spawned an OS task, not that the OS displayed it.
            announced: false,
            signal: target.signal.clone(),
            priority: Some(target.priority),
        },
    );
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

    let Some(route_fingerprint) = whatsapp_route_fingerprint(preferences) else {
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
        tracker.channels.clear();
        tracker.route_fingerprint = Some(route_fingerprint);
        changed = true;
    }

    for channel in &snapshot.channels {
        if !whatsapp_route_configured(channel) {
            for key in dispatch_state_keys(&tracker, &channel.id) {
                tracker.channels.remove(&key);
                changed = true;
            }
            continue;
        }

        let trustworthy = matches!(
            channel.availability,
            AvailabilityDto::Fresh | AvailabilityDto::Delayed
        );
        let targets = if trustworthy {
            current_channel_dispatch_targets(channel, preferences, snapshot)
        } else {
            Vec::new()
        };
        changed |= migrate_legacy_channel_tracker(&mut tracker, channel, snapshot, &targets);

        // Recover the narrow crash boundary between Meta accepting a warning
        // and recording that acceptance. Each item owns its incident, so one
        // accepted earthquake can never promote an unaccepted sibling.
        for key in dispatch_state_keys(&tracker, &channel.id) {
            let Some(mut state) = tracker.channels.get(&key).cloned() else {
                continue;
            };
            if state.active
                && !state.announced
                && let Some(incident_id) = state.incident_id
                && store
                    .outbox_revision_was_accepted(
                        WHATSAPP_ROUTE_ID,
                        incident_id,
                        i64::from(state.revision),
                    )
                    .await
                    .map_err(|error| error.to_string())?
            {
                state.announced = true;
                tracker.channels.insert(key, state);
                changed = true;
            }
        }

        let active_keys = targets
            .iter()
            .map(|target| target.tracker_key.clone())
            .collect::<BTreeSet<_>>();
        for target in &targets {
            let previous = tracker
                .channels
                .get(&target.tracker_key)
                .cloned()
                .unwrap_or_default();
            let view = channel_dispatch_view(channel, target, true);
            if quiet_hours_block(preferences, &view, snapshot)? {
                continue;
            }
            let material = dispatch_material(channel, snapshot, target);
            if previous.active && previous.last_material.as_deref() == Some(material.as_str()) {
                continue;
            }

            let incident_id = if previous.active {
                previous.incident_id.unwrap_or_else(Uuid::now_v7)
            } else {
                Uuid::now_v7()
            };
            let revision = previous.revision.saturating_add(1).max(1);
            let outbox_id = Uuid::now_v7();
            let request = delivery_request_for_dispatch_target(
                preferences,
                snapshot,
                channel,
                target,
                true,
                incident_id,
                outbox_id,
                revision,
                now_ms,
                &material,
            );
            tracker.channels.insert(
                target.tracker_key.clone(),
                ChannelDispatchState {
                    incident_id: Some(incident_id),
                    revision,
                    last_material: Some(material),
                    active: true,
                    announced: previous.active && previous.announced,
                    signal: target.signal.clone(),
                    priority: Some(target.priority),
                },
            );
            commit_whatsapp_transition(
                store,
                &tracker,
                channel,
                target,
                &request,
                incident_id,
                outbox_id,
                revision,
                true,
                &now,
            )
            .await?;
            changed = true;
        }

        // Partial usable coverage may raise a positive signal, but only full
        // coverage can prove that a formerly current item has resolved.
        if !(trustworthy && channel.coverage_complete) {
            continue;
        }
        for key in dispatch_state_keys(&tracker, &channel.id) {
            if active_keys.contains(&key) {
                continue;
            }
            let previous = tracker.channels.get(&key).cloned().unwrap_or_default();
            if !previous.active {
                continue;
            }
            if !previous.announced {
                tracker.channels.insert(
                    key,
                    ChannelDispatchState {
                        last_material: Some("resolved".into()),
                        active: false,
                        ..previous
                    },
                );
                changed = true;
                continue;
            }

            let target = ChannelDispatchTarget {
                tracker_key: key.clone(),
                signal: previous.signal.clone(),
                priority: previous.priority.unwrap_or(channel.priority),
                item_level: key != channel.id,
            };
            let mut view = channel_dispatch_view(channel, &target, false);
            if target.item_level {
                // A resolution is useful, but is not itself an emergency.
                view.priority.urgency = UrgencyDto::Routine;
            }
            if quiet_hours_block(preferences, &view, snapshot)? {
                // Leave the prior incident active so its still-current
                // all-clear can be emitted once quiet hours end.
                continue;
            }
            let incident_id = previous.incident_id.unwrap_or_else(Uuid::now_v7);
            let revision = previous.revision.saturating_add(1).max(1);
            let outbox_id = Uuid::now_v7();
            let request = delivery_request_for_dispatch_target(
                preferences,
                snapshot,
                channel,
                &target,
                false,
                incident_id,
                outbox_id,
                revision,
                now_ms,
                "resolved",
            );
            tracker.channels.insert(
                key,
                ChannelDispatchState {
                    incident_id: Some(incident_id),
                    revision,
                    last_material: Some("resolved".into()),
                    active: false,
                    announced: true,
                    signal: target.signal.clone(),
                    priority: Some(target.priority),
                },
            );
            commit_whatsapp_transition(
                store,
                &tracker,
                channel,
                &target,
                &request,
                incident_id,
                outbox_id,
                revision,
                false,
                &now,
            )
            .await?;
            changed = true;
        }
    }

    if changed || stored.is_none() {
        store
            .set_json(DISPATCH_TRACKER_KEY, &tracker, &now)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn commit_whatsapp_transition(
    store: &Store,
    tracker: &WhatsAppDispatchTracker,
    channel: &ChannelSnapshot,
    target: &ChannelDispatchTarget,
    request: &DeliveryRequest,
    incident_id: Uuid,
    outbox_id: Uuid,
    revision: u32,
    active: bool,
    now: &str,
) -> Result<(), String> {
    store
        .commit_delivery_transition(
            &IncidentRecord {
                id: incident_id,
                channel_id: &channel.id,
                state: if active { "active" } else { "resolved" },
                urgency: urgency_key(target.priority.urgency),
                material_revision: i64::from(revision),
                fingerprint: tracker
                    .channels
                    .get(&target.tracker_key)
                    .and_then(|state| state.last_material.as_deref())
                    .unwrap_or("unknown"),
                payload: &request.notice,
                opened_at: now,
                updated_at: now,
                resolved_at: (!active).then_some(now),
            },
            &OutboxRecord {
                id: outbox_id,
                route_id: WHATSAPP_ROUTE_ID,
                incident_id,
                material_revision: i64::from(revision),
                action: if active {
                    "material_update"
                } else {
                    "resolved"
                },
                request,
                next_attempt_at: now,
                created_at: now,
            },
            DISPATCH_TRACKER_KEY,
            tracker,
            now,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
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
        if !desktop_route_configured(channel) {
            for key in dispatch_state_keys(&tracker, &channel.id) {
                tracker.channels.remove(&key);
                changed = true;
            }
            continue;
        }

        let trustworthy = matches!(
            channel.availability,
            AvailabilityDto::Fresh | AvailabilityDto::Delayed
        );
        let targets = if trustworthy {
            current_channel_dispatch_targets(channel, preferences, snapshot)
        } else {
            Vec::new()
        };
        changed |= migrate_legacy_channel_tracker(&mut tracker, channel, snapshot, &targets);
        let active_keys = targets
            .iter()
            .map(|target| target.tracker_key.clone())
            .collect::<BTreeSet<_>>();

        for target in &targets {
            let view = channel_dispatch_view(channel, target, true);
            if quiet_hours_block(preferences, &view, snapshot)? {
                continue;
            }
            let Some(material) =
                desktop_notification_material_if_due(&tracker, channel, snapshot, target)
            else {
                continue;
            };
            let (title, body) = desktop_notification_copy_for_signal(
                channel,
                target.signal.as_ref(),
                true,
                target.item_level,
            );
            if let Err(error) = app
                .notification()
                .builder()
                .title(bounded_text(&title, 96))
                .body(bounded_text(&body, 320))
                .show()
            {
                first_error.get_or_insert_with(|| {
                    format!("Native notification was not accepted: {error}")
                });
                continue;
            }
            record_desktop_notification(&mut tracker, target, material);
            changed = true;
        }

        if !(trustworthy && channel.coverage_complete) {
            continue;
        }
        for key in dispatch_state_keys(&tracker, &channel.id) {
            if active_keys.contains(&key) {
                continue;
            }
            let previous = tracker.channels.get(&key).cloned().unwrap_or_default();
            if !previous.active {
                continue;
            }
            if previous.announced {
                let item_level = key != channel.id;
                let target = ChannelDispatchTarget {
                    tracker_key: key.clone(),
                    signal: previous.signal.clone(),
                    priority: previous.priority.unwrap_or(channel.priority),
                    item_level,
                };
                let mut view = channel_dispatch_view(channel, &target, false);
                if item_level {
                    view.priority.urgency = UrgencyDto::Routine;
                }
                if quiet_hours_block(preferences, &view, snapshot)? {
                    continue;
                }
                let (title, body) = desktop_notification_copy_for_signal(
                    channel,
                    previous.signal.as_ref(),
                    false,
                    item_level,
                );
                if let Err(error) = app
                    .notification()
                    .builder()
                    .title(bounded_text(&title, 96))
                    .body(bounded_text(&body, 320))
                    .show()
                {
                    first_error.get_or_insert_with(|| {
                        format!("Native notification was not accepted: {error}")
                    });
                    continue;
                }
            }
            tracker.channels.insert(
                key,
                ChannelDispatchState {
                    revision: previous
                        .revision
                        .saturating_add(u32::from(previous.announced)),
                    last_material: Some("resolved".into()),
                    active: false,
                    ..previous
                },
            );
            changed = true;
        }
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
            || reason == "The notice content has been superseded by a newer channel state"
        {
            suppress_and_rearm_current_outbox(store, &lease, &request, &tracker, &now, &reason)
                .await?;
        } else {
            suppress_leased_outbox(store, &lease, &now, &reason).await?;
        }
        return Ok(());
    }
    let quiet_hours_channel = tracker_state_for_request(&tracker, &request)
        .and_then(|(key, state)| {
            state
                .active
                .then(|| current_target_for_key(channel, preferences, snapshot, key))
                .flatten()
                .map(|target| channel_dispatch_view(channel, &target, true))
                .or_else(|| {
                    let mut view = channel.clone();
                    if key != channel.id {
                        view.priority.urgency = UrgencyDto::Routine;
                    }
                    Some(view)
                })
        })
        .unwrap_or_else(|| channel.clone());
    if quiet_hours_block(preferences, &quiet_hours_channel, snapshot)? {
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
    if let Some(current) = tracker.channels.values_mut().find(|current| {
        current.active
            && current.incident_id == Some(request.incident_id)
            && current.revision == request.material_revision
    }) {
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
    let Some(key) = rearmed
        .channels
        .iter()
        .find(|(_, current)| {
            current.incident_id == Some(request.incident_id)
                && current.revision == request.material_revision
        })
        .map(|(key, _)| key.clone())
    else {
        return suppress_leased_outbox(store, lease, now, reason).await;
    };
    let current = rearmed
        .channels
        .get_mut(&key)
        .expect("key came from tracker");

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
    let Some((tracker_key, current)) = tracker_state_for_request(tracker, request) else {
        return Err("The material transition is no longer current".into());
    };
    if i64::from(current.revision) != lease.material_revision {
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
    let target = current_target_for_key(channel, preferences, snapshot, tracker_key);
    let current_active = trustworthy && target.is_some();
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
        dispatch_material(
            channel,
            snapshot,
            target
                .as_ref()
                .expect("active state was checked against a current target"),
        )
    } else {
        "resolved".into()
    };
    if current.last_material.as_deref() != Some(expected_material.as_str())
        || request.deduplication_key != format!("{tracker_key}:{expected_material}")
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
#[cfg(test)]
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
    let target = ChannelDispatchTarget {
        tracker_key: channel.id.clone(),
        signal: channel.signal.clone(),
        priority: channel.priority,
        item_level: false,
    };
    delivery_request_for_dispatch_target(
        preferences,
        snapshot,
        channel,
        &target,
        active,
        incident_id,
        outbox_id,
        revision,
        now_ms,
        material,
    )
}

#[allow(clippy::too_many_arguments)]
fn delivery_request_for_dispatch_target(
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
    channel: &ChannelSnapshot,
    target: &ChannelDispatchTarget,
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
    let signal = target.signal.as_ref();
    let subject = if is_bridge || (!active && !target.item_level) {
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
    } else if let Some(signal) = signal.filter(|_| target.item_level) {
        format!("{} is no longer current.", signal.headline)
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
        deduplication_key: format!("{}:{material}", target.tracker_key),
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
                if !target.item_level && !channel.summary.trim().is_empty() {
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
}

fn urgency_key(urgency: UrgencyDto) -> &'static str {
    match urgency {
        UrgencyDto::Routine => "routine",
        UrgencyDto::HeadsUp => "heads_up",
        UrgencyDto::Action => "action",
        UrgencyDto::Emergency => "emergency",
    }
}

#[cfg(test)]
fn desktop_notification_copy(channel: &ChannelSnapshot, active: bool) -> (String, String) {
    desktop_notification_copy_for_signal(
        channel,
        active.then_some(channel.signal.as_ref()).flatten(),
        active,
        false,
    )
}

fn desktop_notification_copy_for_signal(
    channel: &ChannelSnapshot,
    signal: Option<&brickellstatus_runtime::ChannelSignalDto>,
    active: bool,
    item_level: bool,
) -> (String, String) {
    if !active {
        if item_level && let Some(signal) = signal {
            return (
                format!("{} · Resolved", signal.headline),
                format!(
                    "This notice is no longer current. Source: {}",
                    channel.source_label
                ),
            );
        }
        return (
            format!("{} · Resolved", channel.title),
            format!(
                "The prior active signal has cleared. Source: {}",
                channel.source_label
            ),
        );
    }
    let Some(signal) = signal else {
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
