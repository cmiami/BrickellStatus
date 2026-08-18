# Security policy

BrickellStatus ingests public internet data, can hold messaging credentials,
and can dispatch alerts to physical and personal destinations. Please report
security defects privately so a fix can land before exploitation details are
made public.

## Report a vulnerability

Use GitHub's private vulnerability form:

**[Report a security vulnerability privately](https://github.com/cmiami/BrickellStatus/security/advisories/new)**

Please include only what the maintainers need to reproduce and assess the
problem:

- affected version, commit, platform, and feature;
- impact and a minimal reproduction;
- whether the issue has been exploited or disclosed elsewhere;
- a safe way to contact you for follow-up.

Do **not** open a public issue for an unpatched vulnerability. Do not attach
real access tokens, WhatsApp phone numbers, message bodies, precise private
locations, device identifiers, private feed URLs, SQLite databases, or raw
logs. Replace them with synthetic values and state what was redacted.

Maintainers will acknowledge reports as availability permits, validate the
impact, coordinate a repair and disclosure, and credit the reporter if they
want attribution. This volunteer project does not promise a response-time or
bounty SLA.

## Local data-at-rest boundary

Meta tokens are stored only in the app-data credential file, with owner-only
permissions on Unix and sealed with user-scope DPAPI on Windows, where the
file also inherits the user-profile ACL (the owning user, SYSTEM, and
Administrators).
AISStream keys use that file or the gitignored local `.env` development file.
Neither is intentionally written to preferences, SQLite, logs, diagnostics,
screenshots, or Git.

The rest of the desktop store is **ordinary unencrypted per-user SQLite**.
Saved exact areas, the WhatsApp recipient and consent record, and pending or
retryable message envelopes remain there until the user edits or scrubs them or
retention pruning removes them. This build relies
on operating-system account and disk protection for that database; it does not
provide application-level database encryption. Treat a copied database as
sensitive even when API secrets have been removed.

## E213 Bluetooth boundary

The desktop opens an application-level GATT connection to the public INK1
service. It does not create or require an operating-system pairing or bonded
device record. INK1 does not authenticate or encrypt the RX/TX
characteristics, so a nearby Bluetooth client that knows the published service
UUID can write a frame. CRC32 and `ACK INK1` detect corruption and confirm that
a complete frame arrived; they do not authenticate the sender.

The connected indicator therefore means that this app holds a working GATT
session, not that the display is a trusted endpoint. Even when the app selects
USB, a powered board continues advertising BLE. Treat displayed content as
spoofable and do not use the E213 as the sole authority for a safety or security
decision.

## High-value security boundaries

Reports are especially useful when they involve:

- exposure or persistence of Meta or AISStream secrets outside the
  private app-data credential file;
- WhatsApp delivery without a recorded opt-in, or after unsubscribe;
- webhook receipt acceptance without a valid signature;
- RSS/Atom SSRF, redirect, DNS-rebinding, response-size, or timeout bypasses;
- stale or malformed source data being presented as fresh or `CLEAR`;
- untrusted map, feed, alert, or device text causing script execution;
- BLE behavior that exposes host secrets or exceeds the documented nearby
  unauthenticated frame-write boundary, USB device confusion, or secret leakage
  in diagnostics and exports;
- unsafe release provenance or a dependency-install policy bypass.

## Safety boundary

BrickellStatus is decision-support software. It does not control a bridge,
replace official emergency instructions, guarantee an opening prediction, or
guarantee message delivery. A bad prediction is still a serious correctness
bug, but it is not automatically a security vulnerability; report it through
a normal issue without including private data.
