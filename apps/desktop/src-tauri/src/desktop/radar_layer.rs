// Radar for the map.
//
// The index is fetched here rather than registered as a channel collector on
// purpose. A radar overlay is decoration: if RainViewer is unreachable the map
// loses a texture, and that must not mark the weather channel's coverage
// incomplete and suppress the rain rule, which is what registering it as a
// source would do.
//
// What crosses this boundary is a tile URL template, never imagery. MapLibre
// fetches the tiles itself, straight from the cache host, so the frontend does
// the pixel work the frontend is for.

/// How long a fetched frame is reused. RainViewer publishes a new composite
/// roughly every ten minutes, so re-asking more often than this returns the
/// same answer and spends someone else's bandwidth to do it.
const RADAR_FRAME_CACHE: Duration = Duration::from_secs(4 * 60);
/// Ceiling on how long a panel frame will wait for radar before going out
/// without it. Shorter than the fetcher's own timeout on purpose: the display
/// loop's deadline is what a person actually experiences.
const RADAR_FETCH_BUDGET: Duration = Duration::from_secs(6);

/// Colour scheme 4 is RainViewer's "Rainbow SELEX-SI" — the one that reads as
/// weather rather than as a heat map when laid over a street map.
const RADAR_COLOR_SCHEME: u8 = 4;
/// Smoothed, without the snow overlay: on a 512 px tile the snow mask mostly
/// contributes speckle at Miami latitudes.
const RADAR_SMOOTHING: u8 = 1;
const RADAR_SNOW: u8 = 0;
const RADAR_TILE_SIZE: u16 = 512;
/// Visible credit is mandatory under RainViewer's free terms. Their own docs
/// give "Weather data by RainViewer" as the example wording and
/// `https://www.rainviewer.com/` as the link, so that is what is used verbatim
/// rather than a paraphrase.
const RADAR_ATTRIBUTION: &str = "Weather data by <a href=\"https://www.rainviewer.com/\" target=\"_blank\" rel=\"noreferrer\">RainViewer</a>";

/// The deepest zoom RainViewer documents radar tiles at, as of January 2026.
///
/// Deeper tiles still resolve in practice, so this is declared rather than
/// discovered. It is the raster source's `maxzoom`, not a camera limit: above
/// it MapLibre overzooms the deepest real tile instead of requesting one, so
/// the overlay survives the ceiling starting to be enforced instead of
/// silently vanishing when the reader zooms in.
pub const RADAR_MAX_ZOOM: u8 = 7;

/// What the map needs to draw one radar frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarLayer {
    /// A MapLibre raster tile template, `{z}/{x}/{y}` still unsubstituted.
    pub tile_url_template: String,
    /// When the composite was observed. The map says this out loud rather than
    /// implying the overlay is live.
    pub observed_at: String,
    pub age_seconds: u64,
    /// Highest zoom the tiles exist at. Above this the map overzooms rather
    /// than requesting tiles the service does not serve.
    pub max_zoom: u8,
    /// Required credit for the imagery, rendered on the map.
    pub attribution: String,
}

#[derive(Clone)]
struct CachedRadar {
    layer: Option<RadarLayer>,
    fetched_at: Instant,
}

/// Zoom for the panel composite. Wide enough that an approaching band is
/// visible before it arrives, tight enough that the reader's own coordinate is
/// not a single pixel.
const PANEL_RADAR_ZOOM: u8 = RADAR_MAX_ZOOM;
/// RainViewer's black-and-white scheme. The colour schemes are not luminance
/// ramps — in the rainbow scheme light rain is dark blue and heavy rain is pale
/// yellow — so only this one makes "darker means heavier" true after the
/// conversion to one bit.
const PANEL_RADAR_COLOR_SCHEME: u8 = 0;
const PANEL_RADAR_TILE_SIZE: u16 = 256;

#[derive(Clone)]
struct CachedFigure {
    figure: Option<RadarFigure>,
    key: String,
    fetched_at: Instant,
}

#[derive(Default)]
pub(crate) struct RadarCache {
    latest: TokioMutex<Option<CachedRadar>>,
    panel: TokioMutex<Option<CachedFigure>>,
}

impl RadarCache {
    /// Returns the current frame, fetching one at most every
    /// [`RADAR_FRAME_CACHE`].
    ///
    /// A failure to reach RainViewer is not an error to the caller: the map
    /// simply has no radar to draw, and an overlay is not worth a red banner on
    /// a bridge app.
    async fn layer(&self, collector: &dyn Collector, now_ms: i64) -> Option<RadarLayer> {
        let mut cached = self.latest.lock().await;
        if let Some(entry) = cached.as_ref()
            && entry.fetched_at.elapsed() < RADAR_FRAME_CACHE
        {
            return entry.layer.clone();
        }
        let layer = collector
            .collect(&CollectContext::default())
            .await
            .ok()
            .and_then(|batch| radar_layer_from_items(&batch.items, now_ms));
        *cached = Some(CachedRadar {
            layer: layer.clone(),
            fetched_at: Instant::now(),
        });
        layer
    }
}

impl RadarCache {
    /// The panel's radar figure for a coordinate, or `None` when there is no
    /// recent composite, the fetch fails, or the sky is empty.
    ///
    /// RainViewer serves a tile centred on a latitude and longitude directly,
    /// which is why there is no projection arithmetic anywhere in this path.
    ///
    /// Keyed on frame and coordinate together: a new composite and a moved pin
    /// are both reasons to re-render, and neither is a reason to re-render
    /// while the other holds still. The panel repaints far more often than
    /// either changes.
    async fn panel_figure(
        &self,
        collector: &dyn Collector,
        fetcher: &dyn HttpFetcher,
        latitude: f64,
        longitude: f64,
        now_ms: i64,
    ) -> Option<RadarFigure> {
        let layer = self.layer(collector, now_ms).await?;
        let key = format!("{}|{latitude:.3},{longitude:.3}", layer.observed_at);
        let mut cached = self.panel.lock().await;
        if let Some(entry) = cached.as_ref()
            && entry.key == key
            && entry.fetched_at.elapsed() < RADAR_FRAME_CACHE
        {
            return entry.figure.clone();
        }

        let figure = match panel_tile_url(&layer, latitude, longitude) {
            Some(url) => match fetcher.get(&url, None, &[("accept", "image/png")]).await {
                Ok(response) => match radar_figure_from_png(&response.body) {
                    Ok(figure) => Some(figure),
                    Err(error) => {
                        warn!(%error, "radar composite could not be rendered for the panel");
                        None
                    }
                },
                Err(error) => {
                    warn!(%error, "radar composite could not be fetched for the panel");
                    None
                }
            },
            None => None,
        };
        *cached = Some(CachedFigure {
            figure: figure.clone(),
            key,
            fetched_at: Instant::now(),
        });
        figure
    }
}

/// The coordinate-centred variant of the tile the map draws.
///
/// Built from the same validated host and path, then parsed back into a `Url`
/// so a coordinate that formats into something unexpected cannot become a
/// request.
fn panel_tile_url(layer: &RadarLayer, latitude: f64, longitude: f64) -> Option<Url> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    let base = layer.tile_url_template.split("/{z}/").next()?;
    let base = base.strip_suffix(&format!("/{RADAR_TILE_SIZE}"))?;
    Url::parse(&format!(
        "{base}/{PANEL_RADAR_TILE_SIZE}/{PANEL_RADAR_ZOOM}/{latitude:.4}/{longitude:.4}/{PANEL_RADAR_COLOR_SCHEME}/{RADAR_SMOOTHING}_{RADAR_SNOW}.png"
    ))
    .ok()
}

/// Builds the tile template from a frame pointer.
///
/// The host and path have already been validated by the collector, which is
/// where that check belongs: by the time a URL is being assembled it is too
/// late to ask whether it points where it claims to.
fn radar_layer_from_items(items: &[CollectorItem], now_ms: i64) -> Option<RadarLayer> {
    let frame = items
        .iter()
        .find(|item| item.kind == ItemKind::RadarFrame)?;
    let host = frame.attributes.get("host")?.as_str()?;
    let path = frame.attributes.get("path")?.as_str()?;
    let observed = frame.observed_at?;
    Some(RadarLayer {
        tile_url_template: format!(
            "{host}{path}/{RADAR_TILE_SIZE}/{{z}}/{{x}}/{{y}}/{RADAR_COLOR_SCHEME}/{RADAR_SMOOTHING}_{RADAR_SNOW}.png"
        ),
        observed_at: observed.to_rfc3339(),
        age_seconds: now_ms
            .saturating_sub(observed.timestamp_millis())
            .max(0)
            .unsigned_abs()
            / 1_000,
        max_zoom: RADAR_MAX_ZOOM,
        attribution: RADAR_ATTRIBUTION.into(),
    })
}

/// Whether the reader wants radar at all.
///
/// One switch for both surfaces. The map's own toggle hides the layer for this
/// session; this decides whether it is offered, and whether the panel spends a
/// fetch on it.
fn radar_enabled(preferences: &AppPreferences) -> bool {
    preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Weather)
        .is_none_or(|channel| {
            channel
                .scope
                .get("radarEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
}

#[tauri::command]
async fn get_radar_layer(state: State<'_, DesktopState>) -> Result<Option<RadarLayer>, String> {
    if !radar_enabled(&state.engine.get_preferences().await) {
        return Ok(None);
    }
    Ok(state
        .radar
        .layer(
            state.radar_collector.as_ref(),
            Timestamp::now().as_millisecond(),
        )
        .await)
}

/// The radar figure for the frame about to be drawn, or `None`.
///
/// Returns nothing for a channel that is not weather, so the fetch never
/// happens for a frame that would discard it, and nothing when the reader has
/// no weather area — a composite has to be centred on somewhere.
async fn panel_radar_figure(
    app: &AppHandle,
    snapshot: &AppSnapshot,
    preferences: &AppPreferences,
    channel_id: &str,
) -> Option<RadarFigure> {
    if !radar_enabled(preferences) {
        return None;
    }
    snapshot
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
        .filter(|channel| channel.kind == ChannelKindDto::Weather)?;
    let area = preferences
        .areas
        .iter()
        .find(|area| area.enabled && area.weather_enabled)?;
    let state = app.try_state::<DesktopState>()?;
    state
        .radar
        .panel_figure(
            state.radar_collector.as_ref(),
            state.radar_fetcher.as_ref(),
            area.latitude,
            area.longitude,
            Timestamp::now().as_millisecond(),
        )
        .await
}
