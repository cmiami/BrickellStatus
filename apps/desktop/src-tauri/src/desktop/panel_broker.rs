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
// Two invariants protect the anchor, and both are asserted in tests:
//
// * `rotation_index` advances only when the rotation lane is served. An alert
//   must never consume a rotation slot, or a burst of alerts would silently
//   skip the bridge's home cadence.
// * An alert holds for a bounded time and then returns the panel to rotation.

// BTreeMap, StdMutex and AtomicU64 already come from lib.rs; this file is
// spliced into it and shares its imports. Note `Ordering` there is the atomic
// one, so comparison orderings are spelled out in full below.
use std::collections::BinaryHeap;

/// Longest an alert keeps the panel before rotation resumes.
const ALERT_HOLD: Duration = Duration::from_secs(45);

/// A still-current top-priority state re-enters the queue on this cadence, so a
/// bridge that stays open keeps reclaiming the panel instead of appearing once
/// and then only on its rotation turn.
const ALERT_REASSERT: Duration = Duration::from_secs(180);

/// Score at or above which a state re-asserts itself on that slow cadence. Only
/// a confirmed, road-blocking event needs to keep taking the panel back merely
/// for continuing to be true.
const REASSERT_MIN_SCORE: u16 = 900;

/// How near an event has to be before it is treated as a live warning window.
///
/// Inside this horizon the reader is deciding *now* — whether to turn, whether
/// to leave — and the panel belongs to whatever they are deciding about.
const IMMINENT_HORIZON_MINUTES: u16 = 15;

/// Re-assertion cadence inside that window. Shorter than [`ALERT_HOLD`], so an
/// imminent event reclaims the panel as its own hold expires rather than
/// surrendering the rest of the window to the rotation.
///
/// This is the bug it exists to prevent, and it is the flagship one. A bridge
/// opening in three to eight minutes at better than eighty percent confidence
/// scores 493: `HeadsUp` plus imminence plus the anchor bonus, with no
/// `confirmed` bonus because nothing has been observed yet — that is what
/// *predicted* means. Re-assertion required 900 and `confirmed`, both of which
/// only an already-open bridge satisfies. So the one warning the product exists
/// to give appeared once, held forty-five seconds, and handed the panel back to
/// the rotation for the remaining minutes, which is how a reader with a bridge
/// about to go up in front of them was shown stock prices. "Warn ahead, confirm
/// later" is the first product principle; the panel was doing the opposite.
const ALERT_REASSERT_IMMINENT: Duration = Duration::from_secs(20);

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
    alert_key: String,
    queued_at: tokio::time::Instant,
}

impl Ord for QueuedAlert {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap: highest score first, and among equal scores
        // the *lowest* sequence first, so ties are served in arrival order.
        self.score
            .cmp(&other.score)
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
    Alert { channel_id: String, score: u16 },
    /// The ordinary cadence. Only this advances `rotation_index`.
    Rotation { channel_id: String },
}

#[derive(Debug, Default)]
pub(crate) struct PanelBroker {
    interrupts: StdMutex<BinaryHeap<QueuedAlert>>,
    /// Channel id -> the alert key last enqueued for it. This is what makes an
    /// escalating event re-alert: a changed key is a new alert, an unchanged one
    /// is the same alert still being true.
    seen: StdMutex<BTreeMap<String, String>>,
    /// Last time a channel re-asserted, so a sustained state does not spin.
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

    fn lock_seen(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_reasserted(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, tokio::time::Instant>> {
        self.reasserted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Notices new or changed alerts in a snapshot and queues them.
    ///
    /// Enqueues only what the operator already consented to interrupt for, via
    /// the existing `interrupt_allows` gate. There is deliberately no numeric
    /// floor on top of that: silently ignoring something a user switched on is
    /// worse than showing it.
    pub(crate) fn ingest(&self, snapshot: &AppSnapshot, preferences: &AppPreferences) {
        let now = tokio::time::Instant::now();
        let mut queued_any = false;
        for channel in &snapshot.channels {
            if !panel_eligible(channel, preferences) {
                continue;
            }
            if !channel.active || !interrupt_allows(channel, preferences, snapshot) {
                // A channel that has gone quiet forgets its last alert, so the
                // same condition recurring later is a fresh interrupt.
                self.lock_seen().remove(&channel.id);
                self.lock_reasserted().remove(&channel.id);
                continue;
            }

            let key = alert_key(channel);
            let changed = self.lock_seen().get(&channel.id) != Some(&key);
            // Two reasons to take the panel back while saying the same thing:
            // the event is confirmed and still blocking, or it is close enough
            // that the reader is acting on it right now. The second is the one
            // that matters most and was missing, because imminence is exactly
            // the state in which nothing has been confirmed yet.
            let imminent = channel
                .priority
                .imminence_minutes
                .is_some_and(|minutes| minutes <= IMMINENT_HORIZON_MINUTES);
            let sustained =
                channel.priority.score >= REASSERT_MIN_SCORE && channel.priority.confirmed;
            let cadence = if imminent {
                ALERT_REASSERT_IMMINENT
            } else {
                ALERT_REASSERT
            };
            let due_to_reassert = !changed
                && (imminent || sustained)
                && self
                    .lock_reasserted()
                    .get(&channel.id)
                    .is_none_or(|last| now.duration_since(*last) >= cadence);

            if !changed && !due_to_reassert {
                continue;
            }
            if due_to_reassert {
                self.lock_reasserted().insert(channel.id.clone(), now);
            } else {
                self.lock_seen().insert(channel.id.clone(), key.clone());
                self.lock_reasserted().insert(channel.id.clone(), now);
            }
            self.lock_interrupts().push(QueuedAlert {
                score: channel.priority.score,
                sequence: self
                    .sequence
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                channel_id: channel.id.clone(),
                alert_key: key,
                queued_at: now,
            });
            queued_any = true;
        }
        if queued_any {
            self.wake.notify_waiters();
        }
    }

    fn peek_score(&self) -> Option<u16> {
        self.lock_interrupts().peek().map(|alert| alert.score)
    }

    /// Pops the highest-priority alert that still describes the present.
    fn take_alert(
        &self,
        snapshot: &AppSnapshot,
        preferences: &AppPreferences,
    ) -> Option<(String, u16)> {
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
            if !channel.active
                || !panel_eligible(channel, preferences)
                || alert_key(channel) != alert.alert_key
                || !interrupt_allows(channel, preferences, snapshot)
            {
                continue;
            }
            return Some((alert.channel_id, alert.score));
        }
    }

    /// Chooses the next frame. Alerts win; otherwise the rotation cadence runs.
    pub(crate) fn next(
        &self,
        snapshot: &AppSnapshot,
        preferences: &AppPreferences,
        rotation_index: u64,
    ) -> Option<PanelSelection> {
        if let Some((channel_id, score)) = self.take_alert(snapshot, preferences) {
            return Some(PanelSelection::Alert { channel_id, score });
        }
        rotation_channel(snapshot, preferences, rotation_index).map(|channel| {
            PanelSelection::Rotation {
                channel_id: channel.id.clone(),
            }
        })
    }

    /// Waits out a frame's time on screen, returning early only for something
    /// that outranks it.
    ///
    /// This is the single primitive behind both the rotation dwell and the
    /// post-alert hold. A rotation frame passes `current_score = 0`, so anything
    /// queued preempts it; an alert passes its own score, so only a strictly
    /// higher one displaces the remainder. Equal scores wait their turn, which
    /// is what keeps two comparable events from trading the panel.
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
            if on_screen_for >= MIN_ON_SCREEN
                && self
                    .peek_score()
                    .is_some_and(|queued| queued > current_score)
            {
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

/// Whether a channel may appear on the panel at all.
///
/// Note there is no channel-kind exception here. The previous filter demoted
/// Official, Hurricane, News and Earthquake to active-only regardless of the
/// operator's `presence` choice, which made `Rotation` mean nothing for exactly
/// the channels most worth rotating.
/// Whether a channel has earned a place on the panel right now.
///
/// The anchor is exempt from having to be active, because "Road open" is the
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
    if !channel.enabled || !channel.destinations.contains(&DestinationIdDto::Epaper) {
        return false;
    }
    // Presence already says how much of the panel a channel is entitled to, and
    // the enum draws the distinction explicitly: `Rotation` takes its turn
    // whatever its state, `ActiveOnly` waits until it has something.
    //
    // Requiring `active` on top of that collapsed the two into one. A channel
    // set to Rotation sat out exactly like ActiveOnly, so on a quiet day the
    // home channel was the only eligible one, `others` was empty, and the
    // rotation returned the same frame forever -- which reads as a panel that
    // has stopped rather than one with nothing to add. It also contradicted the
    // product rule that being shown in rotation and being allowed to interrupt
    // are separate decisions.
    match channel.presence {
        SurfacePresence::Off | SurfacePresence::MessagesOnly => false,
        SurfacePresence::Home | SurfacePresence::Rotation => true,
        SurfacePresence::ActiveOnly => {
            channel.active || channel.id == preferences.profile.home_channel_id
        }
    }
}

/// Identity of the thing being alerted about.
///
/// Prefers the signal's band, which is the same string the notification path
/// dedupes on — one identity for both, so the panel and a phone can never
/// disagree about whether something is new.
///
/// `material_key` is the fallback for kinds that carry no band. It hashes the
/// underlying items, which is right for an authored alert and wrong for a
/// measurement: a forecast refresh that moves a number by a tenth produces a
/// fresh key and would re-seize the panel on every poll.
fn alert_key(channel: &ChannelSnapshot) -> String {
    channel
        .signal
        .as_ref()
        .and_then(|signal| signal.band.as_deref())
        .map_or_else(|| channel.material_key.clone(), str::to_owned)
}

/// The ordinary cadence: every eligible channel in turn, home included.
///
/// There is deliberately no "return home every N frames". A periodic detour
/// back to the bridge answered a question nobody was asking on the frames in
/// between, and it did it on a timer rather than when anything changed -- so it
/// both wasted slots and still could not be relied on to be current, because
/// the interesting moment might land in the gap.
///
/// The interrupt lane above already covers it properly: a bridge state change
/// preempts whatever is showing, within milliseconds rather than at the end of
/// a cadence, and holds the panel while it matters. That is the behaviour the
/// detour was approximating badly, so the rotation is now a plain round-robin
/// and the anchor earns the panel by changing rather than by counting.
fn rotation_channel<'a>(
    snapshot: &'a AppSnapshot,
    preferences: &AppPreferences,
    rotation_index: u64,
) -> Option<&'a ChannelSnapshot> {
    let eligible = snapshot
        .channels
        .iter()
        .filter(|channel| panel_eligible(channel, preferences))
        .collect::<Vec<_>>();
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

