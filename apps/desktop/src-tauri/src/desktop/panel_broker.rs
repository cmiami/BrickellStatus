// Two lanes for one panel: a rotation, and interrupts that preempt it.
//
// The display can show one thing. Previously the choice was "first eligible
// channel in preference order", which meant the bridge — index zero — pinned
// the panel for as long as it was interesting, and nothing else was ever seen.
//
// Here the rotation carries the ordinary cadence and a priority queue carries
// anything worth stepping in front of it. The two share one waiting primitive:
// a frame's dwell *is* the interrupt wait, so an alert lands within
// milliseconds instead of at the end of the current dwell, and the hold after
// an alert is itself preemptible so a larger event displaces the remainder
// rather than queueing behind it.
//
// Two invariants keep the sequence predictable, and both are asserted in tests:
//
// * `rotation_index` advances only when the rotation lane is served. An alert
//   must never consume a rotation slot, or a burst of alerts would silently
//   skip current notices.
// * An alert holds for a bounded time and then returns the panel to rotation.

// BTreeMap, StdMutex and AtomicU64 already come from lib.rs; this file is
// spliced into it and shares its imports. Note `Ordering` there is the atomic
// one, so comparison orderings are spelled out in full below.
use std::collections::BinaryHeap;

/// Longest an alert keeps the panel before rotation resumes.
const ALERT_HOLD: Duration = Duration::from_secs(45);

/// How often a still-current bridge or rain warning earns the interrupt lane.
///
/// This is deliberately shorter than [`ALERT_HOLD`]. The next copy is waiting
/// before the current hold ends, so an actionable warning cannot fall through
/// to ordinary slides merely because it has remained true for 45 seconds.
const PRIORITY_ALERT_REASSERT: Duration = Duration::from_secs(30);

/// A queued alert is dropped rather than shown if it waited longer than this;
/// by then it describes a moment that has passed.
const ALERT_MAX_AGE: Duration = Duration::from_secs(120);

/// Minimum time any frame stays on screen, even when something better arrives.
///
/// A full panel write is thousands of bytes over a paced serial link and an
/// e-paper repaint is visibly slow. Without a floor, two close-scoring events
/// would trade the panel faster than it can draw.
const MIN_ON_SCREEN: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueuedAlert {
    score: u16,
    /// Monotonic, for a stable first-in-first-out order among equal scores.
    sequence: u64,
    channel_id: String,
    notice_key: Option<String>,
    alert_key: String,
    /// A new semantic state may replace an equally scored frame immediately.
    /// A periodic reassertion waits for the current alert's normal hold.
    preempts_equal: bool,
    queued_at: tokio::time::Instant,
}

impl Ord for QueuedAlert {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap: highest score first, and among equal scores
        // the *lowest* sequence first, so ties are served in arrival order.
        self.score
            .cmp(&other.score)
            .then_with(|| self.preempts_equal.cmp(&other.preempts_equal))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for QueuedAlert {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// What the panel should show next.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PanelSelection {
    /// Preempts whatever is showing and holds for a bounded time.
    Alert {
        channel_id: String,
        notice_key: Option<String>,
        score: u16,
    },
    /// The ordinary cadence. Only this advances `rotation_index`.
    Rotation {
        channel_id: String,
        notice_key: Option<String>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct PanelBroker {
    interrupts: StdMutex<BinaryHeap<QueuedAlert>>,
    /// Stable event identity -> the alert band last enqueued for it. Authored
    /// items keep separate entries, while bridge and weather keep one semantic
    /// condition entry whose source samples may change without repeating.
    seen: StdMutex<BTreeMap<String, String>>,
    /// Last interrupt enqueue per semantic event. Bridge and rain alerts use
    /// this to retain the panel until they clear without multiplying ordinary
    /// notification repeats.
    reasserted: StdMutex<BTreeMap<String, tokio::time::Instant>>,
    wake: tokio::sync::Notify,
    sequence: AtomicU64,
}

impl PanelBroker {
    fn lock_interrupts(&self) -> std::sync::MutexGuard<'_, BinaryHeap<QueuedAlert>> {
        self.interrupts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_reasserted(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, tokio::time::Instant>> {
        self.reasserted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_seen(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Notices new or changed alerts in a snapshot and queues them.
    ///
    /// Enqueues only active, non-routine material under the engine's automatic
    /// urgency policy. There is no second per-channel interrupt setting here.
    pub(crate) fn ingest(&self, snapshot: &AppSnapshot, preferences: &AppPreferences) {
        let now = tokio::time::Instant::now();
        let mut queued_any = false;
        for channel in &snapshot.channels {
            let tracker_prefix = format!("{}\0", channel.id);
            if !panel_eligible(channel, preferences) || !channel.active {
                // A channel that has gone quiet forgets every event it owned,
                // so a later recurrence is a genuinely fresh onset.
                {
                    let mut seen = self.lock_seen();
                    let mut reasserted = self.lock_reasserted();
                    seen.retain(|tracker_id, _| {
                        tracker_id != &channel.id && !tracker_id.starts_with(&tracker_prefix)
                    });
                    reasserted.retain(|tracker_id, _| {
                        tracker_id != &channel.id && !tracker_id.starts_with(&tracker_prefix)
                    });
                }
                continue;
            }

            let candidates = alert_candidates(channel);
            let current_ids = candidates
                .iter()
                .map(|candidate| candidate.tracker_id.clone())
                .collect::<BTreeSet<_>>();
            let ready = {
                let mut seen = self.lock_seen();
                let mut reasserted = self.lock_reasserted();
                seen.retain(|tracker_id, _| {
                    !tracker_id.starts_with(&tracker_prefix)
                        || current_ids.contains(tracker_id.as_str())
                });
                reasserted.retain(|tracker_id, _| {
                    !tracker_id.starts_with(&tracker_prefix)
                        || current_ids.contains(tracker_id.as_str())
                });
                candidates
                    .into_iter()
                    .filter_map(|candidate| {
                        let changed =
                            seen.get(&candidate.tracker_id) != Some(&candidate.alert_key);
                        let due_to_reassert = !changed
                            && candidate.retains_priority
                            && reasserted.get(&candidate.tracker_id).is_none_or(|last| {
                                now.duration_since(*last) >= PRIORITY_ALERT_REASSERT
                            });
                        if !changed && !due_to_reassert {
                            return None;
                        }
                        if changed {
                            seen.insert(candidate.tracker_id.clone(), candidate.alert_key.clone());
                        }
                        reasserted.insert(candidate.tracker_id.clone(), now);
                        Some((candidate, changed))
                    })
                    .collect::<Vec<_>>()
            };

            // Onsets and material status changes interrupt immediately. Bridge
            // and rain alerts additionally keep a single interrupt queued so
            // routine rotation cannot take the panel while they remain active.
            for (candidate, changed) in ready {
                self.lock_interrupts().push(QueuedAlert {
                    score: candidate.score,
                    sequence: self
                        .sequence
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    channel_id: channel.id.clone(),
                    notice_key: candidate.notice_key.map(str::to_owned),
                    alert_key: candidate.alert_key,
                    preempts_equal: changed,
                    queued_at: now,
                });
                queued_any = true;
            }
        }
        if queued_any {
            self.wake.notify_waiters();
        }
    }

    fn has_preempting_alert(&self, current_score: u16) -> bool {
        self.lock_interrupts().peek().is_some_and(|alert| {
            alert.score > current_score
                || (alert.score == current_score && alert.preempts_equal)
        })
    }

    /// Pops the highest-priority alert that still describes the present.
    fn take_alert(
        &self,
        snapshot: &AppSnapshot,
        preferences: &AppPreferences,
    ) -> Option<(String, Option<String>, u16)> {
        let now = tokio::time::Instant::now();
        loop {
            let alert = self.lock_interrupts().pop()?;
            if now.duration_since(alert.queued_at) > ALERT_MAX_AGE {
                continue;
            }
            let Some(channel) = snapshot
                .channels
                .iter()
                .find(|channel| channel.id == alert.channel_id)
            else {
                continue;
            };
            // Superseded: the channel moved on while this sat in the queue, so
            // showing it would report a state that is no longer true.
            let still_current = alert_candidates(channel).into_iter().any(|candidate| {
                candidate.notice_key == alert.notice_key.as_deref()
                    && candidate.alert_key == alert.alert_key
            });
            if !channel.active || !panel_eligible(channel, preferences) || !still_current {
                continue;
            }
            return Some((alert.channel_id, alert.notice_key, alert.score));
        }
    }

    /// Chooses the next frame. Alerts win; otherwise the rotation cadence runs.
    pub(crate) fn next(
        &self,
        snapshot: &AppSnapshot,
        preferences: &AppPreferences,
        rotation_index: u64,
    ) -> Option<PanelSelection> {
        if let Some((channel_id, notice_key, score)) = self.take_alert(snapshot, preferences) {
            return Some(PanelSelection::Alert {
                channel_id,
                notice_key,
                score,
            });
        }
        rotation_channel(snapshot, preferences, rotation_index).map(|entry| {
            PanelSelection::Rotation {
                channel_id: entry.channel.id.clone(),
                notice_key: entry.notice_key.map(str::to_owned),
            }
        })
    }

    /// Waits out a frame's time on screen, returning early only for something
    /// that outranks it.
    ///
    /// This is the single primitive behind both the rotation dwell and the
    /// post-alert hold. A rotation frame passes `current_score = 0`, so anything
    /// queued preempts it. A new material state may also replace an equally
    /// scored alert; a periodic reassertion waits for the normal hold to end.
    pub(crate) async fn wait_or_preempt(&self, current_score: u16, dwell: Duration) {
        let started = tokio::time::Instant::now();
        let deadline = started + dwell;
        loop {
            // Registered before peeking: a notification that lands between the
            // peek and the await would otherwise be lost and the alert would
            // sit in the queue until the dwell expired.
            let notified = self.wake.notified();
            let now = tokio::time::Instant::now();
            let on_screen_for = now.duration_since(started);
            if on_screen_for >= MIN_ON_SCREEN && self.has_preempting_alert(current_score) {
                return;
            }
            if now >= deadline {
                return;
            }
            // Never sleep past the point where preemption becomes allowed.
            let until_preemptible = MIN_ON_SCREEN.saturating_sub(on_screen_for);
            let remaining = deadline.saturating_duration_since(now);
            let sleep_for = if until_preemptible.is_zero() {
                remaining
            } else {
                remaining.min(until_preemptible)
            };
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(sleep_for) => {
                    if sleep_for == remaining {
                        return;
                    }
                }
            }
        }
    }

    pub(crate) fn alert_hold() -> Duration {
        ALERT_HOLD
    }
}

/// Whether a channel has earned a place on the panel right now.
///
/// The anchor is exempt from having to be active, because "Bridge closed" is the
/// answer this app exists to give and is worth the whole screen. Nothing else
/// is: a channel with nothing to report has only its own empty state to show,
/// and a panel that spends a slot saying no feed items matched has taken the
/// bridge off the screen to display nothing.
///
/// This restores a filter Phase 1 removed. The old one was keyed on channel
/// *kind*, which was arbitrary and made `presence: Rotation` a lie for half the
/// roster; the rule was never "news cannot rotate", it was "nothing with
/// nothing to say gets the screen".
fn panel_eligible(channel: &ChannelSnapshot, preferences: &AppPreferences) -> bool {
    if !channel.enabled {
        return false;
    }
    // One relevance rule replaces presence modes, destinations and empty
    // reservations. The home decision is the quiet fallback; every other
    // channel exists on the panel only while it has something current to say.
    channel.active || channel.id == preferences.profile.home_channel_id
}

struct AlertCandidate<'a> {
    tracker_id: String,
    notice_key: Option<&'a str>,
    alert_key: String,
    score: u16,
    retains_priority: bool,
}

/// Current interrupt candidates, with stable semantic deduplication.
///
/// * Bridge movement samples share one condition identity. Urgency and ETA
///   bands are material, while raw AIS position churn is not.
/// * Weather forecast-bin ids are deliberately ignored. Its band already says
///   whether rain/wind timing or intensity changed enough to matter.
/// * Authored items keep their own identities, and severity/urgency is included
///   so a Severe -> Extreme update can interrupt as an escalation.
fn alert_candidates(channel: &ChannelSnapshot) -> Vec<AlertCandidate<'_>> {
    if channel.kind == ChannelKindDto::Bridge {
        return (!matches!(channel.priority.urgency, UrgencyDto::Routine))
            .then(|| AlertCandidate {
                tracker_id: format!("{}\0condition", channel.id),
                notice_key: None,
                alert_key: format!(
                    "bridge:{:?}:{}",
                    channel.priority.urgency,
                    imminence_band(channel.priority.imminence_minutes)
                ),
                score: channel.priority.score,
                retains_priority: true,
            })
            .into_iter()
            .collect();
    }

    if channel.notices.is_empty() {
        return (!matches!(channel.priority.urgency, UrgencyDto::Routine))
            .then(|| AlertCandidate {
                tracker_id: format!("{}\0condition", channel.id),
                notice_key: None,
                alert_key: channel
                    .signal
                    .as_ref()
                    .and_then(|signal| signal.band.as_deref())
                    .map_or_else(|| channel.material_key.clone(), str::to_owned),
                score: channel.priority.score,
                retains_priority: channel.kind == ChannelKindDto::Weather
                    && channel
                        .signal
                        .as_ref()
                        .is_some_and(|signal| signal_contains_rain(signal.band.as_deref())),
            })
            .into_iter()
            .collect();
    }

    channel
        .notices
        .iter()
        .filter(|notice| !matches!(notice.priority.urgency, UrgencyDto::Routine))
        .map(|notice| {
            let (tracker_id, alert_key) = if channel.kind == ChannelKindDto::Weather {
                (
                    format!("{}\0condition", channel.id),
                    notice
                        .signal
                        .band
                        .as_deref()
                        .map_or_else(|| notice.key.clone(), |band| format!("weather:{band}")),
                )
            } else {
                let severity = notice
                    .signal
                    .severity
                    .as_deref()
                    .unwrap_or("unspecified")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_ascii_lowercase();
                (
                    format!("{}\0{}", channel.id, notice.key),
                    format!("{}:{:?}:{severity}", notice.key, notice.priority.urgency),
                )
            };
            AlertCandidate {
                tracker_id,
                notice_key: Some(notice.key.as_str()),
                alert_key,
                score: notice.priority.score,
                retains_priority: channel.kind == ChannelKindDto::Weather
                    && signal_contains_rain(notice.signal.band.as_deref()),
            }
        })
        .collect()
}

/// ETA bands are changes in what the driver should do; second-by-second AIS
/// movement inside one band is not. This mirrors the weather lead bands.
fn imminence_band(minutes: Option<u16>) -> &'static str {
    match minutes {
        Some(0..=5) => "0-5",
        Some(6..=15) => "6-15",
        Some(16..=30) => "16-30",
        Some(31..=60) => "31-60",
        Some(_) => "60+",
        None => "unknown",
    }
}

fn signal_contains_rain(band: Option<&str>) -> bool {
    band.is_some_and(|band| band.split('+').any(|part| part.starts_with("rain-")))
}

/// The ordinary cadence: every current notice gets one slot, highest priority
/// first, plus the home decision as the quiet fallback.
///
/// There is deliberately no "return home every N frames" and no repeat count.
/// The index advances once per served slide and wraps the current set. A new
/// urgent event uses the interrupt lane above, then remains here while relevant.
#[derive(Clone, Copy)]
struct RotationChannel<'a> {
    channel: &'a ChannelSnapshot,
    notice_key: Option<&'a str>,
    score: u16,
    ordinal: usize,
}

impl std::ops::Deref for RotationChannel<'_> {
    type Target = ChannelSnapshot;

    fn deref(&self) -> &Self::Target {
        self.channel
    }
}

fn rotation_channel<'a>(
    snapshot: &'a AppSnapshot,
    preferences: &AppPreferences,
    rotation_index: u64,
) -> Option<RotationChannel<'a>> {
    let mut eligible = Vec::new();
    for channel in snapshot
        .channels
        .iter()
        .filter(|channel| panel_eligible(channel, preferences))
    {
        if channel.notices.is_empty() {
            eligible.push(RotationChannel {
                channel,
                notice_key: None,
                score: channel.priority.score,
                ordinal: 0,
            });
        } else {
            eligible.extend(
                channel
                    .notices
                    .iter()
                    .enumerate()
                    .map(|(ordinal, notice)| RotationChannel {
                        channel,
                        notice_key: Some(notice.key.as_str()),
                        score: notice.priority.score,
                        ordinal,
                    }),
            );
        }
    }
    eligible.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.channel.id.cmp(&right.channel.id))
            .then_with(|| left.ordinal.cmp(&right.ordinal))
    });
    if eligible.is_empty() {
        return None;
    }
    let offset = usize::try_from(rotation_index % eligible.len() as u64).unwrap_or(0);
    eligible.get(offset).copied()
}

/// Whether to attempt a proof frame on this pass.
///
/// The panel refuses unforced frames until one has been acknowledged, which is
/// the right invariant -- never blast a display whose wire has not been proven.
/// The bug it caused was that arming was *manual*: it resets on every connect
/// and disconnect, and only an explicit test frame set it, so after any
/// reconnect the panel silently showed nothing until someone pressed a button.
///
/// Pulled out as a pure function because `ActiveDisplay` wraps concrete
/// transports with no seam to fake, and this decision is the part worth testing.
pub(crate) fn should_prove_now(
    has_active: bool,
    armed: bool,
    now: tokio::time::Instant,
    next_attempt_at: tokio::time::Instant,
) -> bool {
    has_active && !armed && now >= next_attempt_at
}

/// Backoff for repeated proof failures, matching the reconnect ladder.
pub(crate) fn prove_backoff(failures: u32) -> Duration {
    Duration::from_secs(match failures {
        0 | 1 => 5,
        2 => 15,
        3 => 30,
        _ => 60,
    })
}
