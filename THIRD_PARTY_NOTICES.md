# Third-party notices

BrickellStatus is distributed under the **MIT** license, but that choice does
not replace the licenses or attribution requirements of third-party software,
fonts, icons, services, or data. `LICENSE-APACHE` remains in the repository as
the canonical Apache-2.0 text for bundled dependencies, not as a license for
this project.

This concise notice calls out the UI assets and map resources a user can see.
The exact resolved software graph remains recorded in `Cargo.lock` and
`apps/console/package-lock.json`; release builds must retain the license texts
that accompany their bundled dependencies.

## Bundled interface software and assets

| Component | Use in BrickellStatus | License and notice |
|---|---|---|
| [MapLibre GL JS](https://github.com/maplibre/maplibre-gl-js) | Interactive global location map renderer | BSD-3-Clause. Copyright © MapLibre contributors. The distribution includes portions under compatible licenses; retain the complete [upstream license file](https://github.com/maplibre/maplibre-gl-js/blob/main/LICENSE.txt). |
| [Barlow Condensed](https://github.com/jpt/barlow) | Condensed display and instrument labels, packaged through Fontsource | SIL Open Font License 1.1. Copyright © 2017 The Barlow Project Authors. See the upstream [`OFL.txt`](https://github.com/jpt/barlow/blob/master/OFL.txt). |
| [Public Sans](https://github.com/uswds/public-sans) | Interface and explanatory text, packaged through Fontsource | SIL Open Font License 1.1. Copyright © 2015 The Public Sans Project Authors. See the upstream [`LICENSE.md`](https://github.com/uswds/public-sans/blob/master/LICENSE.md). |
| [Lucide](https://github.com/lucide-icons/lucide) | Interface symbols through `@lucide/svelte` | ISC. Copyright © Lucide Icons and Contributors. Some icons derive from Feather and remain MIT-licensed; retain Lucide's complete [combined license notice](https://github.com/lucide-icons/lucide/blob/main/LICENSE). |

## Vendored source

| Component | Use in BrickellStatus | License and notice |
|---|---|---|
| [btleplug](https://github.com/deviceplug/btleplug) (droidplug Java) | The Java half of the Android Bluetooth backend, copied verbatim into `apps/desktop/src-tauri/android/droidplug/java/` and compiled into the Android app. Includes the bundled `io.github.gedgygedgy.rust` jni-utils classes. | MIT / Apache-2.0 / BSD-3-Clause, at the recipient's option. Copyright © the btleplug contributors. These sources are not published to Maven, which is why they are vendored; see the directory's `README.md` for provenance and the resync script. |

No third-party trademark is granted by the project license. Names are used
only to identify their respective software, data, service, or hardware.

## Network map service and data

Map tiles are requested at runtime; they are **not** copied into the app or
DMG. The map must keep this attribution visible and legible:

> OpenFreeMap © OpenMapTiles Data from OpenStreetMap

- [OpenFreeMap](https://openfreemap.org/) provides the public tile/style
  service under its current [Terms of Service](https://openfreemap.org/tos/)
  and publishes the project under MIT. It offers no availability SLA.
- The style uses the [OpenMapTiles](https://openmaptiles.org/) schema; see its
  [license and attribution terms](https://openmaptiles.org/license/).
- Map data is © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright)
  and is available under the Open Data Commons Open Database License (ODbL).

### Radar overlay

Radar composites come from [RainViewer](https://www.rainviewer.com/) via its
public weather-maps API. The app fetches an index of frame locations and hands
MapLibre a tile URL; imagery is never copied into the app or DMG. The map must
keep this attribution visible:

> Weather data by RainViewer

RainViewer requires that credit with a link back to `https://www.rainviewer.com/`,
and the radar source carries it as its MapLibre attribution so the map renders it
alongside the base-map credit whenever the overlay is on.

The free tier is offered for **personal, educational, and small-scale community
use**, with no key, no SLA, and a documented limit of 100 requests per IP per
minute. This app polls the index at most once every four minutes and fetches one
panel composite per radar frame, which is far inside that. Anyone redistributing
this app for commercial use needs to arrange terms with RainViewer directly.

MapLibre renders source attribution from the active style. Do not hide,
obscure, crop, or replace the attribution control in the desktop or browser
surface. Exported screenshots, video, and print must carry the attribution
required by the relevant providers and data licenses.

## Maintainer release check

Before publishing a binary:

1. confirm this file still matches the locked versions and actual assets;
2. retain each bundled dependency's complete license text where its license
   requires binary-distribution notice;
3. confirm the live map exposes OpenFreeMap/OpenMapTiles/OpenStreetMap
   attribution at every supported window size, and RainViewer attribution
   whenever the radar overlay is on;
4. regenerate dependency/license reports after any lockfile change.

This file is an attribution record, not legal advice or an exhaustive
substitute for the license files shipped by every dependency.
