# Tender's Log — Heltec E213 firmware

This firmware turns the Heltec Vision Master E213 into a host-driven, one-bit
status display. The Rust host renders the complete 250 × 122 image and sends it
over native USB CDC or Bluetooth Low Energy. The board only validates and
displays frames.

## Hardware scope

- Original Vision Master E213 (`V1`) and the revised `V1.1` panel are supported.
- This project does **not** initialize or use Wi-Fi.
- This project does **not** initialize or use LoRa. It is appropriate for the
  no-LoRa E213 monitor board; no radio module is required.
- USB and BLE are the only host transports.
- The board has no runtime controls to operate. Power it and use the desktop
  app for discovery, connection, rotation, and proof-frame actions.

## Build and flash

[PlatformIO](https://platformio.org/) is required. Libraries and the Espressif
platform are pinned in `platformio.ini` for reproducible builds.

Original E213 (`V1`, the default):

```sh
pio run --project-dir firmware/e213 -e vision-master-e213
pio run --project-dir firmware/e213 -e vision-master-e213 --target upload
```

E213 `V1.1`:

```sh
pio run --project-dir firmware/e213 -e vision-master-e213-v11
pio run --project-dir firmware/e213 -e vision-master-e213-v11 --target upload
```

Serial diagnostics:

```sh
pio device monitor --baud 115200
```

Select the correct panel revision explicitly. A wrong revision can produce a
blank or scrambled display even though transport acknowledgements succeed.

## Backward-compatible wire contract

The firmware intentionally retains the sibling `e213` project's contract:

- BLE advertised name: `InkDock E213`
- service UUID: `8b7a0000-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- RX UUID: `8b7a0001-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- TX UUID: `8b7a0002-4f4b-4a9b-9d6e-1d0c1a2b3c4d`
- host packet: `INK1`, exactly 3,922 bytes
- payload: 3,904 bytes, 32 bytes per row, MSB-first black bits
- replies: `READY INK1`, `ACK INK1`, or a descriptive `NACK ...`

USB serial is 115,200 baud. BLE clients must subscribe to TX before writing
packet chunks to RX so that a fast acknowledgement cannot be missed.
