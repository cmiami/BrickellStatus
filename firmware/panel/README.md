# BrickellStatus — Heltec e-paper panel firmware

This firmware turns a Heltec Vision Master board into a host-driven, one-bit
status display. The Rust host renders the complete image and sends it over
native USB CDC or Bluetooth Low Energy. The board only identifies itself,
validates frames, and displays them.

## Hardware scope

- Vision Master **E213**, 250 × 122 — original (`V1`) and revised (`V1.1`) panels.
- Vision Master **E290**, 296 × 128.
- This project does **not** initialize or use Wi-Fi.
- This project does **not** initialize or use LoRa. It is appropriate for the
  no-LoRa monitor boards; no radio module is required.
- USB and BLE are the only host transports.
- The board has no runtime controls to operate. Power it and use the desktop
  app for discovery, connection, rotation, and proof-frame actions.

## The board identifies itself

Nobody is asked which board they plugged in. The two boards carry the same six
display GPIOs with the roles permuted:

| pin | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| E213 | BUSY | DC | RST | CLK | CS | MOSI |
| E290 | MOSI | CLK | CS | DC | RST | BUSY |

Only `BUSY` is driven by the panel; the other five are panel inputs on either
board. So at boot the firmware reads pins 1 and 6 twice each — once biased low,
once biased high — and asks which one is being *driven*. A panel output
overrides the internal pull and answers the same way both times; a pin wired to
the panel's `MOSI` input floats and follows the pull. The pin that answers names
the board. Neither line is ever driven, so the probe cannot contend with a panel
output, and it reads no levels or timings: the E213 `V1` controller holds `BUSY`
low while busy and the others hold it high, so a probe that read polarity would
identify one board and mis-identify another.

Only a positive identification of the *other* board stops a build from driving.
A probe that cannot tell — neither line answering — falls back to the board the
image was written for, which is how this firmware behaved before it could ask at
all. The probe is allowed to redirect a flash; it is not allowed to take a
working panel out of service over an unfamiliar reading.

The display library binds one board's pinout and controller per image, so each
environment below carries one of them. A build that lands on a board it can see
is the wrong one leaves the panel alone and says so, and the desktop app writes
the right image without asking anything:

```text
READY INK1 0x0 0 <build> E290 <board-id> FW2 MISMATCH
```

The E213's two panel revisions share the same wiring and board probe, but their
controllers use opposite `BUSY` polarity. The wrong image blocks before the
decoder loop and never answers `READY INK1`. The app writes one internal E213
image, sends the repeatable identity query, and tries the other image once only
when READY is absent. It remembers only the image that answers.

## Build and flash

[PlatformIO](https://platformio.org/) is required. Libraries and the Espressif
platform are pinned in `platformio.ini` for reproducible builds.

```sh
# E213, revised V1.1 panel (the default)
pio run --project-dir firmware/panel -e vision-master-e213-v11
pio run --project-dir firmware/panel -e vision-master-e213-v11 --target upload

# E213, original V1 panel
pio run --project-dir firmware/panel -e vision-master-e213

# E290
pio run --project-dir firmware/panel -e vision-master-e290
```

Serial diagnostics:

```sh
pio device monitor --baud 115200
```

The probe prints what it found on every attempt, which is the first thing to
read when a board comes up blank:

```text
PROBE attempt=0 pin1=driven pin6=floating
READY INK1 250x122 3904 9f3c2ab E213 26B4 FW2
```

## Wire contract

The frame contract is unchanged apart from carrying whichever geometry the
attached panel has. It has always announced its dimensions; only the host's
assumption that there was one answer had to go.

- BLE advertised name: `BrickellStatus XXXX`, where `XXXX` is the stable
  four-character board code shown on the waiting screen
- service UUID: `8b7a0000-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- RX UUID: `8b7a0001-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- TX UUID: `8b7a0002-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- host packet: `INK1`, 3,922 bytes on the E213 and 4,754 on the E290
- payload: 3,904 bytes at 32 bytes per row, or 4,736 at 37, MSB-first black bits
- replies: `READY INK1 ...`, `ACK INK1`, or a descriptive `NACK ...`
- identity query: a single `?` byte between frames repeats the full
  `READY INK1 ...` banner

The trailing `FW<n>` is the monotonic firmware release shared by E213 and
E290. It is the only field used to decide upgrade direction: lower means the
bundled firmware is newer, higher means the desktop app must be updated and
must not downgrade the panel. The adjacent source build identifies exact bytes
but is never ordered. Clean builds use the last firmware Git revision; dirty
builds add a deterministic content digest so two different working trees cannot
claim the same identity. Any firmware-affecting release must increment
`version.txt`; changing a source build without incrementing it is intentionally
reported as a different, unordered build rather than an automatic update.

A host that sends the geometry the attached panel does not have is answered
`NACK SIZE` rather than shown a smear.

The board advertises without a time limit whenever no BLE client is connected,
restarts advertising after a disconnect, and checks the radio every two seconds
so a transient stopped-advertising state repairs itself. The e-paper waiting
screen is drawn once at boot and persists by design; it identifies the panel but
is not a live connection or advertising indicator.

The TX characteristic holds the full banner rather than a bare `READY`, because
over Bluetooth that line is the only place the geometry is spoken and the host
has to know which panel it is drawing for before it draws. USB serial is
115,200 baud. BLE clients must subscribe to TX before writing packet chunks to
RX so that a fast acknowledgement cannot be missed.

The repeated identity query also makes E213 controller recovery objective. The
two E213 images use opposite BUSY polarity; the wrong one blocks before the
main loop and cannot answer `?`, even though the glass may retain a perfectly
readable image from older firmware. The desktop app tries the other E213 image
once when READY is absent and remembers only the image that answers.
