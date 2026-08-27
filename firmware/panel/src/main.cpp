#include <Arduino.h>
#include <NimBLEDevice.h>
#include <esp_mac.h>
#include <esp_task_wdt.h>
#include <heltec-eink-modules.h>

#include <algorithm>
#include <array>
#include <cstring>

// Which board this is, worked out by the board rather than by a person.
//
// The Vision Master E213 and E290 carry the same six display GPIOs with their
// roles permuted:
//
//   pin      1      2      3      4      5      6
//   E213     BUSY   DC     RST    CLK    CS     MOSI
//   E290     MOSI   CLK    CS     DC     RST    BUSY
//
// Five of those are panel inputs whichever board is attached; only BUSY is a
// panel output. So the probe never drives pins 1 or 6 — it asks which of them
// is being *driven*, reading each one first with a pull-down and then with a
// pull-up. A panel output overrides the weak internal pull and answers the same
// way twice; a pin wired to the panel's MOSI input floats and follows the pull.
// The pin that answers names the board.
//
// Deliberately not a polarity or timing test: the E213 V1 panel is a Fitipower
// controller that holds BUSY LOW while busy and the others are SSD parts that
// hold it HIGH, so a probe reading levels would identify one board and
// mis-identify another.
//
// The display library binds its pinout at compile time, so a build carries one
// board's driver and the probe's job is to confirm it is on that board. When it
// is not, the firmware says which board it actually found and leaves the panel
// alone: writing an E213 waveform onto E290 pins is how a board comes back with
// a scrambled screen and no explanation. The desktop app reads that line and
// writes the right build, so being wrong costs a minute rather than a diagnosis.

namespace {

struct PanelWiring {
  const char *name;
  uint16_t width;   // Landscape width, which is what the host draws.
  uint16_t height;  // Landscape height.
  uint8_t busy;
};

constexpr PanelWiring kE213{"E213", 250, 122, 1};
constexpr PanelWiring kE290{"E290", 296, 128, 6};
constexpr PanelWiring kWirelessPaper{"WPAPER", 250, 122, 7};

#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
// Wireless Paper's display bus is half-duplex: GPIO2 is driven while sending a
// command, then released while the controller answers on the same wire. Heltec
// uses this 0x2F query in its V1.1/V1.2 firmware to choose the display driver.
constexpr uint8_t kWirelessPaperVextPin = 45;
constexpr uint8_t kWirelessPaperDataPin = 2;
constexpr uint8_t kWirelessPaperClockPin = 3;
constexpr uint8_t kWirelessPaperCsPin = 4;
constexpr uint8_t kWirelessPaperDcPin = 5;
constexpr uint8_t kWirelessPaperResetPin = 6;

enum class WirelessPaperController : uint8_t {
  Lcmen2r13efc1,
  E0213a367,
};
#endif

#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
constexpr const PanelWiring &kBuiltFor = kWirelessPaper;
#elif defined(Vision_Master_E290)
constexpr const PanelWiring &kBuiltFor = kE290;
#else
constexpr const PanelWiring &kBuiltFor = kE213;
#endif

/// Vision Master peripheral power, used only by its non-driving board probe.
constexpr uint8_t kVextPin = 18;

constexpr uint16_t kWidth = kBuiltFor.width;
constexpr uint16_t kHeight = kBuiltFor.height;
constexpr size_t kStride = (kWidth + 7) / 8;
constexpr size_t kPayloadSize = kStride * kHeight;
constexpr size_t kHeaderSize = 18;
constexpr size_t kPacketSize = kHeaderSize + kPayloadSize;
constexpr uint8_t kFlagFullRefresh = 0x01;
constexpr uint32_t kFrameAssemblyTimeoutMs = 1000;
constexpr uint32_t kBleAdvertisingHealthCheckMs = 2000;
constexpr uint32_t kLoopWatchdogTimeoutSeconds = 30;

// Battery telemetry follows each PCB's own gated divider. Wireless Paper's
// display BUSY is GPIO7, so reusing the Vision Master pins here would contend
// with the panel on every READY/ACK. Its schematic instead places an active-LOW
// 10k/10k divider on GPIO19/20.
#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
constexpr uint8_t kBatteryAdcControlPin = 19;
constexpr uint8_t kBatteryAdcPin = 20;
constexpr uint8_t kBatteryAdcControlActive = LOW;
// `analogReadMilliVolts` applies the ESP32-S3 ADC calibration/attenuation;
// this factor is only the schematic's nominal 10k/10k divider ratio.
constexpr uint16_t kBatteryMultiplierMilli = 2000;
constexpr auto kBatteryAdcAttenuation = ADC_11db;
#else
constexpr uint8_t kBatteryAdcControlPin = 46;
constexpr uint8_t kBatteryAdcPin = 7;
constexpr uint8_t kBatteryAdcControlActive = HIGH;
constexpr uint16_t kBatteryMultiplierMilli = 5047;  // 4.9 * 1.03
constexpr auto kBatteryAdcAttenuation = ADC_2_5db;
#endif
constexpr size_t kBatterySampleCount = 9;
constexpr uint32_t kBatterySampleIntervalMs = 30000;
// A single-cell LiPo below this level needs attention. The wider clear point
// keeps a battery resting near the boundary from repeatedly changing state.
constexpr uint16_t kLowBatteryEnterMillivolts = 3400;
constexpr uint16_t kLowBatteryExitMillivolts = 3550;
constexpr uint16_t kPlausibleBatteryMinimumMillivolts = 2500;
constexpr uint16_t kPlausibleBatteryMaximumMillivolts = 5000;
constexpr size_t kLowBatteryAckLength = sizeof("ACK INK1 BAT0000 LOW") - 1;

// Keep the enriched low-battery ACK inside the default 20-byte ATT payload.
static_assert(kLowBatteryAckLength <= 20);

// The warning the panel draws for itself, along its bottom edge.
//
// Size 2 for the same reason the waiting screen uses it: this has to be read by
// someone walking past a board with no host attached, which is the whole case
// it exists for. The strip lands on the band the host already draws at the foot
// of a frame, so black with white text reads as that band saying something else
// rather than as damage.
constexpr uint8_t kBatteryBannerTextSize = 2;
constexpr uint16_t kBatteryBannerGlyphWidth = 6 * kBatteryBannerTextSize;
constexpr uint16_t kBatteryBannerGlyphHeight = 8 * kBatteryBannerTextSize;
constexpr uint16_t kBatteryBannerRule = 1;
constexpr uint16_t kBatteryBannerPadding = 2;
constexpr uint16_t kBatteryBannerHeight = kBatteryBannerRule +
                                          kBatteryBannerPadding +
                                          kBatteryBannerGlyphHeight +
                                          kBatteryBannerPadding;
constexpr uint16_t kBatteryBannerTop = kHeight - kBatteryBannerHeight;
constexpr char kBatteryBannerLabel[] = "LOW BATTERY";
constexpr uint16_t kBatteryBannerSeparator = 2 * kBatteryBannerGlyphWidth;

// Proved against the widest reading the plausibility window admits rather than
// against whatever a battery happened to show, so the line cannot wrap out of
// its strip and across a host frame on either panel.
constexpr uint16_t kBatteryBannerWidestWidth =
    (sizeof(kBatteryBannerLabel) - 1 + sizeof("5.00V") - 1) *
        kBatteryBannerGlyphWidth +
    kBatteryBannerSeparator;

static_assert(kBatteryBannerWidestWidth <= kWidth);
// A strip, not a takeover: the panel is still a status display.
static_assert(kBatteryBannerHeight * 4 < kHeight);

constexpr bool shouldRestoreBleAdvertising(uint8_t connectedCount,
                                           bool advertising) {
  return connectedCount == 0 && !advertising;
}

static_assert(shouldRestoreBleAdvertising(0, false));
static_assert(!shouldRestoreBleAdvertising(1, false));
static_assert(!shouldRestoreBleAdvertising(0, true));

// GATT identifiers intentionally remain compatible with deployed INK1 firmware
// and existing desktop senders. Only the advertised name carries the board
// code, so a host scanning for a panel learns which one it found before it
// connects.
constexpr char kServiceUuid[] = "8b7a0000-4f4b-4a9b-9d6e-1d0c1a2b3c4d";
constexpr char kRxUuid[] = "8b7a0001-4f4b-4a9b-9d6e-1d0c1a2b3c4d";
constexpr char kTxUuid[] = "8b7a0002-4f4b-4a9b-9d6e-1d0c1a2b3c4d";

/// Built only once the probe agrees this is the right board.
///
/// A global display object would construct before `setup()` runs, and its
/// constructor powers the rail, resets the panel, claims the SPI pins and
/// writes a blank frame. On the board this build is not for, one of those pins
/// is the panel's own BUSY output, so the driver and the panel would drive the
/// same line against each other before anything had established which board
/// this is. Nothing touches a pin here until the probe has answered.
BaseDisplay *display = nullptr;

/// The board the probe actually found, or null if nothing answered.
const PanelWiring *attached = nullptr;
/// Whether this build can drive what is attached.
bool driving = false;

char bannerLine[112] = "READY";

/// Four hex characters naming this particular board, and nothing else.
///
/// Every board running this firmware used to advertise the same Bluetooth name,
/// because the name was built from the panel model alone. Two boards on one
/// desk were then indistinguishable in a picker, and pinning a connection to a
/// name could not pick between them.
///
/// The last two octets of the factory MAC settle it. They are unique per board,
/// and they survive a routine firmware update, which a random suffix would not.
/// The host already reads this MAC while flashing, so it can work out the same
/// four characters without asking the board for them.
char boardId[5] = "0000";

void composeBoardId() {
  uint8_t mac[6] = {0};
  // The factory MAC, read straight from efuse. This starts no radio: the board
  // brings up neither Wi-Fi nor LoRa, and reading the address does not change
  // that.
  if (esp_read_mac(mac, ESP_MAC_WIFI_STA) != ESP_OK) return;
  snprintf(boardId, sizeof(boardId), "%02X%02X", mac[4], mac[5]);
}

#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
WirelessPaperController detectWirelessPaperController() {
  // Vext is active LOW. Give the rail the same settling time used by Heltec's
  // reference query before touching the controller.
  pinMode(kWirelessPaperVextPin, OUTPUT);
  digitalWrite(kWirelessPaperVextPin, LOW);
  delay(100);

  pinMode(kWirelessPaperClockPin, OUTPUT);
  pinMode(kWirelessPaperDcPin, OUTPUT);
  pinMode(kWirelessPaperCsPin, OUTPUT);
  pinMode(kWirelessPaperResetPin, OUTPUT);
  digitalWrite(kWirelessPaperClockPin, LOW);
  digitalWrite(kWirelessPaperDcPin, HIGH);
  digitalWrite(kWirelessPaperCsPin, HIGH);

  digitalWrite(kWirelessPaperResetPin, LOW);
  delay(20);
  digitalWrite(kWirelessPaperResetPin, HIGH);
  delay(20);

  digitalWrite(kWirelessPaperDcPin, LOW);
  digitalWrite(kWirelessPaperCsPin, LOW);
  pinMode(kWirelessPaperDataPin, OUTPUT);

  uint8_t command = 0x2F;
  for (uint8_t bit = 0; bit < 8; ++bit) {
    digitalWrite(kWirelessPaperDataPin,
                 (command & 0x80) != 0 ? HIGH : LOW);
    command <<= 1;
    digitalWrite(kWirelessPaperClockPin, HIGH);
    delayMicroseconds(1);
    digitalWrite(kWirelessPaperClockPin, LOW);
    delayMicroseconds(1);
  }
  delay(10);

  digitalWrite(kWirelessPaperDcPin, HIGH);
  pinMode(kWirelessPaperDataPin, INPUT_PULLUP);
  uint8_t chipId = 0;
  for (int8_t bit = 7; bit >= 0; --bit) {
    digitalWrite(kWirelessPaperClockPin, LOW);
    delayMicroseconds(1);
    digitalWrite(kWirelessPaperClockPin, HIGH);
    delayMicroseconds(1);
    if (digitalRead(kWirelessPaperDataPin) == HIGH) {
      chipId |= static_cast<uint8_t>(1U << bit);
    }
  }
  digitalWrite(kWirelessPaperCsPin, HIGH);

  // Heltec's documented split: E0213A367 answers with Chip ID bits 01;
  // LCMEN2R13EFC1 does not. V1.0's SSD1680 also answers 01, so this firmware
  // intentionally makes no unsupported claim that it can distinguish V1.0.
  const WirelessPaperController controller =
      (chipId & 0x03) == 0x01
          ? WirelessPaperController::E0213a367
          : WirelessPaperController::Lcmen2r13efc1;
  Serial.printf("PANEL controller-id=0x%02X driver=%s\n", chipId,
                controller == WirelessPaperController::E0213a367
                    ? "E0213A367"
                    : "LCMEN2R13EFC1");
  return controller;
}
#endif

BaseDisplay *makeDisplay() {
#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
  if (detectWirelessPaperController() ==
      WirelessPaperController::E0213a367) {
    return new EInkDisplay_WirelessPaperV1_2();
  }
  return new EInkDisplay_WirelessPaperV1_1();
#elif defined(Vision_Master_E290)
  return new DEPG0290BNS800();
#elif BRICKELLSTATUS_PANEL_V11
  return new EInkDisplay_VisionMasterE213V1_1();
#else
  return new EInkDisplay_VisionMasterE213();
#endif
}

std::array<uint8_t, kPayloadSize> pendingFrame{};
volatile bool framePending = false;
volatile bool pendingFullRefresh = true;
portMUX_TYPE frameMux = portMUX_INITIALIZER_UNLOCKED;
NimBLECharacteristic *txCharacteristic = nullptr;
NimBLEServer *bleServer = nullptr;
NimBLEAdvertising *bleAdvertising = nullptr;
uint32_t lastBleAdvertisingHealthAt = 0;
bool loopWatchdogArmed = false;
bool batterySampled = false;
bool batteryKnown = false;
bool batteryLow = false;
uint16_t batteryMillivolts = 0;
uint32_t lastBatterySampleAt = 0;

/// Whether the last thing drawn on this panel carried the warning strip.
///
/// The glass cannot be read back, so what was last put there is the only fact
/// worth keeping. Comparing it against `batteryLow` is the entire trigger:
/// there is no request flag to lose and no timer to fire, and the hysteresis in
/// `refreshBatteryTelemetry` already stops a battery resting on the threshold
/// from repainting every thirty seconds.
bool batteryBannerDrawn = false;

/// Whether `pendingFrame` holds a frame that actually reached the glass.
///
/// The buffer is value-initialised, so before the first host frame it is a
/// legitimate all-white image rather than "nothing". Re-rendering it to take
/// the strip back off would quietly replace the waiting screen with a blank
/// panel.
bool hostFrameRendered = false;

void composeBanner();

void setupBatteryTelemetry() {
  pinMode(kBatteryAdcControlPin, OUTPUT);
  digitalWrite(kBatteryAdcControlPin, !kBatteryAdcControlActive);
  pinMode(kBatteryAdcPin, INPUT);
  analogReadResolution(12);
  analogSetPinAttenuation(kBatteryAdcPin, kBatteryAdcAttenuation);
}

/// Takes one bounded, median-filtered reading when the prior one is stale.
///
/// The voltage divider is powered only around the nine ADC conversions. A
/// calibrated ADC millivolt reading is then scaled by the board's divider. A
/// disconnected battery on these boards can produce a low phantom voltage, so
/// anything outside the plausible single-cell range is reported as unknown.
bool refreshBatteryTelemetry() {
  const uint32_t now = millis();
  if (batterySampled && now - lastBatterySampleAt < kBatterySampleIntervalMs) {
    return false;
  }
  batterySampled = true;
  lastBatterySampleAt = now;

  digitalWrite(kBatteryAdcControlPin, kBatteryAdcControlActive);
  delay(10);
  std::array<uint32_t, kBatterySampleCount> samples{};
  for (uint32_t &sample : samples) {
    sample = analogReadMilliVolts(kBatteryAdcPin);
    delay(1);
  }
  digitalWrite(kBatteryAdcControlPin, !kBatteryAdcControlActive);

  std::sort(samples.begin(), samples.end());
  const uint32_t pinMillivolts = samples[samples.size() / 2];
  const uint32_t scaled =
      (pinMillivolts * kBatteryMultiplierMilli + 500) / 1000;
  if (scaled < kPlausibleBatteryMinimumMillivolts ||
      scaled > kPlausibleBatteryMaximumMillivolts) {
    batteryKnown = false;
    batteryMillivolts = 0;
    return true;
  }

  batteryKnown = true;
  batteryMillivolts = static_cast<uint16_t>(scaled);
  if (batteryLow) {
    if (batteryMillivolts >= kLowBatteryExitMillivolts) batteryLow = false;
  } else if (batteryMillivolts <= kLowBatteryEnterMillivolts) {
    batteryLow = true;
  }
  return true;
}

void appendBatteryTelemetry(char *line, size_t capacity,
                            bool compactLowMarker = false) {
  if (!batteryKnown) return;
  const size_t used = std::strlen(line);
  if (used >= capacity) return;
  snprintf(line + used, capacity - used, " BAT%u%s", batteryMillivolts,
           batteryLow ? (compactLowMarker ? " LOW" : " LOWBAT") : "");
}

uint16_t readLe16(const uint8_t *value) {
  return static_cast<uint16_t>(value[0]) |
         (static_cast<uint16_t>(value[1]) << 8);
}

uint32_t readLe32(const uint8_t *value) {
  return static_cast<uint32_t>(value[0]) |
         (static_cast<uint32_t>(value[1]) << 8) |
         (static_cast<uint32_t>(value[2]) << 16) |
         (static_cast<uint32_t>(value[3]) << 24);
}

uint32_t crc32(const uint8_t *data, size_t length) {
  uint32_t crc = 0xFFFFFFFFu;
  for (size_t i = 0; i < length; ++i) {
    crc ^= data[i];
    for (uint8_t bit = 0; bit < 8; ++bit) {
      const uint32_t mask = -(crc & 1u);
      crc = (crc >> 1) ^ (0xEDB88320u & mask);
    }
  }
  return ~crc;
}

void acknowledge(const char *message) {
  const bool ready = std::strncmp(message, "READY INK1", 10) == 0;
  const bool ack = std::strcmp(message, "ACK INK1") == 0;
  if (ready || ack) refreshBatteryTelemetry();
  // ACK/NACK is transient. The characteristic's idle value must always be the
  // current full READY banner so reconnecting clients can recover geometry,
  // version, board identity, and battery state without waiting for a notify.
  composeBanner();

  char enriched[48];
  const char *reply = message;
  if (ready) {
    reply = bannerLine;
  } else if (ack) {
    snprintf(enriched, sizeof(enriched), "ACK INK1");
    appendBatteryTelemetry(enriched, sizeof(enriched), true);
    reply = enriched;
  }

  Serial.println(reply);
  if (txCharacteristic != nullptr) {
    txCharacteristic->setValue(reply);
    // Pass the response bytes explicitly so restoring the readable value below
    // cannot race an asynchronously queued notification.
    txCharacteristic->notify(reinterpret_cast<const uint8_t *>(reply),
                             std::strlen(reply));
    txCharacteristic->setValue(bannerLine);
  }
}

/// Whether something on the other end of this pin is driving it.
///
/// Read once biased low and once biased high. A driven line ignores the
/// internal pull and answers the same way twice; a floating one follows it.
bool pinIsDriven(uint8_t pin) {
  pinMode(pin, INPUT_PULLDOWN);
  delayMicroseconds(600);
  const int low_bias = digitalRead(pin);
  pinMode(pin, INPUT_PULLUP);
  delayMicroseconds(600);
  const int high_bias = digitalRead(pin);
  pinMode(pin, INPUT);
  return low_bias == high_bias;
}

/// Identifies the attached board, or returns null if neither answered.
const PanelWiring *probePanel() {
  pinMode(kVextPin, OUTPUT);
  digitalWrite(kVextPin, HIGH);
  delay(50);  // Peripheral rail settling, as the vendor platform also waits.

  // Repeated because a panel mid-refresh can hold its BUSY line at the level
  // the internal pull happens to agree with for one reading.
  for (uint8_t attempt = 0; attempt < 5; ++attempt) {
    const bool e213_driven = pinIsDriven(kE213.busy);
    const bool e290_driven = pinIsDriven(kE290.busy);
    Serial.printf("PROBE attempt=%u pin%u=%s pin%u=%s\n", attempt, kE213.busy,
                  e213_driven ? "driven" : "floating", kE290.busy,
                  e290_driven ? "driven" : "floating");
    if (e213_driven && !e290_driven) return &kE213;
    if (e290_driven && !e213_driven) return &kE290;
    delay(40);
  }
  Serial.println("PROBE inconclusive; keeping this build's own board");
  return nullptr;
}

bool submitPacket(const uint8_t *packet, size_t length) {
  if (!driving) {
    acknowledge("NACK WRONG BUILD");
    return false;
  }
  if (length != kPacketSize || std::memcmp(packet, "INK1", 4) != 0) {
    acknowledge("NACK FORMAT");
    return false;
  }

  const uint16_t width = readLe16(packet + 4);
  const uint16_t height = readLe16(packet + 6);
  const uint8_t flags = packet[8];
  const uint8_t reserved = packet[9];
  const uint32_t payloadLength = readLe32(packet + 10);
  const uint32_t expectedCrc = readLe32(packet + 14);
  const uint8_t *payload = packet + kHeaderSize;

  // The frame has to be drawn for the panel that is actually here. A host
  // sending the other geometry is told so rather than shown a smear.
  if (width != kWidth || height != kHeight || payloadLength != kPayloadSize ||
      reserved != 0 || (flags & ~kFlagFullRefresh) != 0) {
    acknowledge("NACK SIZE");
    return false;
  }
  if (crc32(payload, payloadLength) != expectedCrc) {
    acknowledge("NACK CRC");
    return false;
  }

  portENTER_CRITICAL(&frameMux);
  std::memcpy(pendingFrame.data(), payload, kPayloadSize);
  pendingFullRefresh = (flags & kFlagFullRefresh) != 0;
  framePending = true;
  portEXIT_CRITICAL(&frameMux);
  return true;
}

class PacketDecoder {
 public:
  void push(const uint8_t *data, size_t length) {
    resetIfTimedOut();
    for (size_t i = 0; i < length; ++i) {
      pushByte(data[i]);
    }
  }

  void poll() { resetIfTimedOut(); }

  /// Whether a packet is part-assembled, i.e. a host is mid-sentence.
  bool assembling() const { return used_ > 0; }

 private:
  std::array<uint8_t, kPacketSize> buffer_{};
  size_t used_ = 0;
  uint32_t lastByteAt_ = 0;

  void resetIfTimedOut() {
    if (used_ == 0 || millis() - lastByteAt_ <= kFrameAssemblyTimeoutMs) return;
    used_ = 0;
    acknowledge("NACK TRUNCATED");
  }

  void pushByte(uint8_t value) {
    lastByteAt_ = millis();
    // A host may have missed the one-time boot line while USB re-enumerated.
    // `?` asks for the same identity again, but only between packets: a byte in
    // an INK1 header or payload can never turn into an out-of-band reply. A
    // wrong E213 controller build is blocked before `loop()` and stays silent,
    // which gives the flasher an objective signal to try the other image once.
    if (used_ == 0 && value == '?') {
      acknowledge(bannerLine);
      return;
    }
    if (used_ < 4) {
      static constexpr uint8_t magic[4] = {'I', 'N', 'K', '1'};
      if (value == magic[used_]) {
        buffer_[used_++] = value;
      } else {
        used_ = value == magic[0] ? 1 : 0;
        if (used_ == 1) buffer_[0] = value;
      }
      return;
    }

    buffer_[used_++] = value;
    if (used_ == kPacketSize) {
      submitPacket(buffer_.data(), used_);
      used_ = 0;
    }
  }
};

PacketDecoder serialDecoder;
PacketDecoder bleDecoder;

class RxCallbacks final : public NimBLECharacteristicCallbacks {
  void onWrite(NimBLECharacteristic *characteristic,
               NimBLEConnInfo &connInfo) override {
    (void)connInfo;
    const std::string &value = characteristic->getValue();
    bleDecoder.push(reinterpret_cast<const uint8_t *>(value.data()), value.size());
  }
};

class ServerCallbacks final : public NimBLEServerCallbacks {
  void onDisconnect(NimBLEServer *server, NimBLEConnInfo &connInfo,
                    int reason) override {
    (void)server;
    (void)connInfo;
    (void)reason;
    NimBLEDevice::startAdvertising();
  }
};

/// Keeps a disconnected board discoverable even if the BLE host reports a
/// transient advertising failure. The waiting image is e-ink and therefore
/// cannot prove the radio is still live; this check asks the radio itself.
void maintainBleAdvertising() {
#if BRICKELLSTATUS_ENABLE_BLE
  const uint32_t now = millis();
  if (now - lastBleAdvertisingHealthAt < kBleAdvertisingHealthCheckMs) return;
  lastBleAdvertisingHealthAt = now;

  if (bleServer == nullptr || bleAdvertising == nullptr) return;
  if (!shouldRestoreBleAdvertising(bleServer->getConnectedCount(),
                                   bleAdvertising->isAdvertising())) {
    return;
  }

  if (bleAdvertising->start()) {
    Serial.println("BLE advertising restored");
  } else {
    Serial.println("BLE advertising retry failed");
  }
#endif
}

void setupBle() {
#if BRICKELLSTATUS_ENABLE_BLE
  // The name a person will actually look for, which is this project's name and
  // the four characters that separate one board from another.
  //
  // It does not fit in the advertisement. That packet holds 31 bytes, and the
  // flags plus this service's 128-bit UUID spend 21 of them, leaving room for
  // about eight characters -- which is why putting any useful panel name there
  // came back as "Data length exceeded" and left the board advertising no
  // name. The scan response is a second 31-byte packet for exactly this, and
  // every scanner asks for it, so the name goes there and the UUID stays where
  // a filtering scanner can see it without asking twice.
  char name[32];
  snprintf(name, sizeof(name), "BrickellStatus %s", boardId);
  NimBLEDevice::init(name);
  NimBLEDevice::setPower(ESP_PWR_LVL_P3);
  bleServer = NimBLEDevice::createServer();
  bleServer->setCallbacks(new ServerCallbacks());
  NimBLEService *service = bleServer->createService(kServiceUuid);
  NimBLECharacteristic *rx = service->createCharacteristic(
      kRxUuid, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
  txCharacteristic = service->createCharacteristic(
      kTxUuid, NIMBLE_PROPERTY::READ | NIMBLE_PROPERTY::NOTIFY);
  rx->setCallbacks(new RxCallbacks());
  // The banner rather than a bare "READY": over Bluetooth this is the only
  // place the geometry is spoken, and a host that cannot read it does not know
  // which panel to draw for.
  txCharacteristic->setValue(bannerLine);
  service->start();

  bleAdvertising = NimBLEDevice::getAdvertising();
  bleAdvertising->addServiceUUID(kServiceUuid);
  bleAdvertising->enableScanResponse(true);
  NimBLEAdvertisementData scanResponse;
  scanResponse.setName(name);
  bleAdvertising->setScanResponseData(scanResponse);
  if (!bleAdvertising->start()) {
    Serial.println("BLE initial advertising failed; retrying in background");
  }
  lastBleAdvertisingHealthAt = millis();
#endif
}

/// Reboots a board whose main loop stops making progress.
///
/// The e-paper glass keeps its last pixels without power, so a board stalled
/// in the display driver looks exactly like a healthy board waiting for a
/// connection. The BLE host runs beside the Arduino loop, but its advertising
/// repair above cannot run if that loop is wedged. A generous timeout covers a
/// full e-paper refresh and turns that otherwise permanent state into a clean
/// reboot.
void armLoopWatchdog() {
  if (esp_task_wdt_init(kLoopWatchdogTimeoutSeconds, true) != ESP_OK) {
    Serial.println("Main-loop watchdog initialization failed");
    return;
  }
  if (esp_task_wdt_add(nullptr) != ESP_OK) {
    Serial.println("Main-loop watchdog subscription failed");
    return;
  }
  loopWatchdogArmed = true;
}

/// Composites the low-battery strip over whatever was just drawn.
///
/// Called last by every path that puts anything on this panel, which is the
/// point: the panel keeps a strip of its own glass, so a host frame loses that
/// strip rather than the warning losing the panel. A board ran flat unattended
/// with nothing on the glass to say so, and a warning that lives only in the
/// wire protocol cannot be read by someone walking past a panel with no host.
void drawBatteryBanner() {
  // Recorded before the early return: "drew nothing" is as much a record of
  // what the glass carries as the strip is, and it is what lets the loop notice
  // that a recovered battery has left a stale warning behind.
  batteryBannerDrawn = batteryLow;
  if (!batteryLow) return;

  // A rule in the background colour, because the host's own band at the foot of
  // a frame is black with white text too. Without it a dark frame and this
  // strip merge into one shape and the warning stops reading as something the
  // panel added.
  display->fillRect(0, kBatteryBannerTop, kWidth, kBatteryBannerRule, WHITE);
  display->fillRect(0, kBatteryBannerTop + kBatteryBannerRule, kWidth,
                    kBatteryBannerHeight - kBatteryBannerRule, BLACK);

  // The reading is dropped rather than invented when the divider last answered
  // implausibly. `batteryLow` keeps its state through an unreadable sample, so
  // the warning stays true where the number is not known, and 0.00V would be
  // the one thing on this strip that was a lie.
  char reading[8] = "";
  if (batteryKnown) {
    snprintf(reading, sizeof(reading), "%u.%02uV", batteryMillivolts / 1000,
             (batteryMillivolts % 1000) / 10);
  }

  const uint16_t labelWidth =
      (sizeof(kBatteryBannerLabel) - 1) * kBatteryBannerGlyphWidth;
  const uint16_t readingWidth =
      std::strlen(reading) * kBatteryBannerGlyphWidth;
  const uint16_t total =
      readingWidth == 0 ? labelWidth
                        : labelWidth + kBatteryBannerSeparator + readingWidth;
  const uint16_t left = (kWidth - total) / 2;
  const uint16_t textTop =
      kBatteryBannerTop + kBatteryBannerRule + kBatteryBannerPadding;

  // Wrapping is turned off rather than merely proved impossible above: a line
  // that wrapped would put white text on the host's frame outside this strip,
  // which is the one failure here that would read as a broken panel.
  display->setTextWrap(false);
  display->setTextSize(kBatteryBannerTextSize);
  display->setTextColor(WHITE);
  display->setCursor(left, textTop);
  display->print(kBatteryBannerLabel);
  if (readingWidth > 0) {
    // The separator is drawn rather than typed. The built-in font shifts every
    // code point from 176 upward unless CP437 mode is on, so a literal middle
    // dot in this source would print as some other glyph.
    const uint16_t dot = kBatteryBannerTextSize + 1;
    display->fillRect(left + labelWidth + (kBatteryBannerSeparator - dot) / 2,
                      textTop + (kBatteryBannerGlyphHeight - dot) / 2, dot, dot,
                      WHITE);
    display->setCursor(left + labelWidth + kBatteryBannerSeparator, textTop);
    display->print(reading);
  }
  display->setTextWrap(true);
}

void drawWaitingScreen() {
  // This screen is no longer drawn only once -- a recovering battery brings it
  // back -- so the waveform has to be settled first, or the host frame it
  // replaces ghosts underneath it.
  display->fastmodeOff();
  display->landscape();
  display->clearMemory();
  display->setTextColor(BLACK);
  display->setTextSize(2);
  display->setCursor(14, 18);
  display->print("BrickellStatus");
  display->setTextSize(1);
  display->setCursor(15, 50);
  display->print("READY / USB + BLE");
  display->setCursor(15, 68);
  display->print("BLUETOOTH NAME");
  // The exact string this board advertises, printed at the size of something
  // meant to be read across a desk. Matching a board to its entry in a list is
  // the whole job of this screen, so the name is shown verbatim rather than
  // described -- what is on the glass is what appears in the picker.
  //
  // The exact string this board advertises, at the size of something meant to
  // be read across a desk. Matching a board to an entry in a list is the whole
  // job of this screen, so it is printed verbatim rather than described.
  //
  // Nothing else earns the space. "NO WI-FI / NO LORA" named two things the
  // board never does, and the panel model and pixel count are facts a reader
  // cannot act on -- the app already knows both, and neither helps anyone
  // choose between two boards on a desk.
  display->setTextSize(2);
  display->setCursor(8, 86);
  char advertised[32];
  snprintf(advertised, sizeof(advertised), "BrickellStatus %s", boardId);
  display->print(advertised);
  drawBatteryBanner();
  display->update();
}

/// The line the host identifies this board by.
///
/// `READY INK1 <w>x<h> <payload> <build> <board> <id> FW<version> [MISMATCH]
/// [BAT<mV> [LOWBAT]]`
///
/// The geometry is what this firmware can draw right now, so a build sitting on
/// the wrong board reports none: there is nothing it can correctly accept. The
/// board name is what the probe found, which is what the app needs in order to
/// write the build that belongs here.
void composeBanner() {
  // With nothing identified, this build's own board is the honest answer: it is
  // what the firmware is about to drive, and what the app should keep writing.
  const char *board = attached != nullptr ? attached->name : kBuiltFor.name;
  if (driving) {
    snprintf(bannerLine, sizeof(bannerLine),
             "READY INK1 %ux%u %u %s %s %s FW%u",
             kWidth, kHeight, static_cast<unsigned>(kPayloadSize),
             BRICKELLSTATUS_BUILD_ID, board, boardId,
             BRICKELLSTATUS_FIRMWARE_VERSION);
  } else {
    snprintf(bannerLine, sizeof(bannerLine),
             "READY INK1 0x0 0 %s %s %s FW%u MISMATCH",
             BRICKELLSTATUS_BUILD_ID, board, boardId,
             BRICKELLSTATUS_FIRMWARE_VERSION);
  }
  appendBatteryTelemetry(bannerLine, sizeof(bannerLine));
}

/// Keeps the value read by a future BLE connection reasonably fresh without
/// waking the ADC divider continuously or sending unsolicited notifications.
void maintainBatteryTelemetry() {
  if (!refreshBatteryTelemetry()) return;
  composeBanner();
  if (txCharacteristic != nullptr) txCharacteristic->setValue(bannerLine);
}

/// Draws one host frame into the page buffer, warning strip included.
///
/// Shared because the frame that arrives and the frame that has to be put back
/// after the warning clears are the same image drawn the same way; two copies
/// of this loop would be two chances for them to disagree about what the panel
/// is showing.
void drawHostFrame(const std::array<uint8_t, kPayloadSize> &frame) {
  display->clearMemory();
  for (uint16_t y = 0; y < kHeight; ++y) {
    for (uint16_t x = 0; x < kWidth; ++x) {
      const size_t offset = static_cast<size_t>(y) * kStride + x / 8;
      const bool black = (frame[offset] & (0x80u >> (x % 8))) != 0;
      display->drawPixel(x, y, black ? BLACK : WHITE);
    }
  }
  drawBatteryBanner();
}

void renderPendingFrame() {
  if (!framePending || !driving || display == nullptr) return;

  std::array<uint8_t, kPayloadSize> frame{};
  bool fullRefresh;
  portENTER_CRITICAL(&frameMux);
  std::memcpy(frame.data(), pendingFrame.data(), kPayloadSize);
  fullRefresh = pendingFullRefresh;
  framePending = false;
  portEXIT_CRITICAL(&frameMux);

  if (fullRefresh) {
    display->fastmodeOff();
  } else {
    display->fastmodeOn();
  }
  drawHostFrame(frame);
  display->update();
  hostFrameRendered = true;
  acknowledge("ACK INK1");
}

/// Puts back on the glass what the panel should currently be showing.
///
/// Adding the strip could be done by drawing over the retained page buffer,
/// since that buffer mirrors the glass -- but taking it away cannot: the strip
/// is in that buffer too, and the only way back is to build what was underneath
/// it again. Doing both directions the same way keeps one drawing path, and the
/// pixel loop costs milliseconds against a refresh that costs seconds.
///
/// Which image is underneath is the question `hostFrameRendered` answers.
void repaintPanel() {
  if (!hostFrameRendered) {
    drawWaitingScreen();
    return;
  }

  // Copied out under the lock for the reason `renderPendingFrame` copies: the
  // loop below runs tens of thousands of iterations, and holding the frame lock
  // across it would keep interrupts off on this core for milliseconds.
  std::array<uint8_t, kPayloadSize> frame{};
  portENTER_CRITICAL(&frameMux);
  std::memcpy(frame.data(), pendingFrame.data(), kPayloadSize);
  portEXIT_CRITICAL(&frameMux);

  // The full waveform in both directions. A large solid band appearing under a
  // partial refresh comes out mottled, and one disappearing leaves its ghost.
  display->fastmodeOff();
  drawHostFrame(frame);
  display->update();
  // Deliberately no acknowledgement: nobody sent this frame, and a spurious
  // ACK INK1 would be a lie to a host counting them.
}

/// Puts the warning on the glass, or takes it off, when the two disagree.
///
/// Nothing here is on a timer. The comparison is against what was last drawn,
/// so a sample that only confirms what is already showing costs no refresh at
/// all, and the hysteresis in `refreshBatteryTelemetry` means a battery resting
/// on the threshold cannot flip the panel back and forth. Across a battery's
/// life this repaints twice.
///
/// This is also the only path that can warn a panel with nobody connected,
/// which is the case it exists for.
void maintainBatteryBanner() {
  if (!driving || display == nullptr) return;
  if (batteryLow == batteryBannerDrawn) return;

  // Deferred while a host is mid-sentence on serial. A repaint takes the panel
  // for the length of a full refresh, the serial drain lives in this same loop,
  // and the decoder gives a host one second between bytes -- so starting one
  // now is how a frame in flight becomes NACK TRUNCATED. The strip can wait for
  // the gap. BLE needs no such gate: its bytes are assembled in the radio's own
  // task and reach `pendingFrame` regardless of what this task is blocked on.
  if (framePending || serialDecoder.assembling()) return;

  repaintPanel();
}

}  // namespace

void setup() {
  Serial.setRxBufferSize(kPacketSize + 64);
  Serial.begin(115200);
  Serial.setTimeout(50);
  armLoopWatchdog();

  composeBoardId();
  setupBatteryTelemetry();
  refreshBatteryTelemetry();
#if defined(BRICKELLSTATUS_WIRELESS_PAPER)
  // The CP2102 USB bridge identifies the Wireless Paper PCB to the host. The
  // firmware then queries the display controller itself in makeDisplay(), so
  // neither the reader nor the app has to choose a panel revision.
  attached = &kWirelessPaper;
  driving = true;
#else
  attached = probePanel();
  // Only a positive identification of the *other* board stops this build from
  // driving. A probe that could not tell falls back to the board this image was
  // written for, which is how the firmware behaved before it could ask at all:
  // the probe is allowed to redirect a flash, never to take a working panel out
  // of service because it read an unfamiliar line.
  driving = attached == nullptr || attached == &kBuiltFor;
#endif
  composeBanner();

  // Bring up the radio before touching the e-paper driver. If a damaged panel
  // or BUSY line ever stalls an update, the board remains discoverable until
  // the watchdog gives it a clean restart instead of becoming a permanent
  // image with no reachable firmware behind it.
  setupBle();
  if (driving) {
    // Only now: the panel this build knows how to talk to is the one attached.
    display = makeDisplay();
    drawWaitingScreen();
  }

  // The build id lets the host tell whether the device is running the firmware
  // the app ships. Without it a working board can only be reported as "unknown
  // build", never as up to date, so the app would have no basis for offering a
  // flash and no basis for staying quiet.
  Serial.println(bannerLine);
}

void loop() {
  if (loopWatchdogArmed) esp_task_wdt_reset();
  serialDecoder.poll();
  bleDecoder.poll();
  uint8_t chunk[256];
  while (Serial.available() > 0) {
    const size_t count = Serial.readBytes(chunk, sizeof(chunk));
    if (count == 0) break;
    serialDecoder.push(chunk, count);
  }

  renderPendingFrame();
  maintainBatteryTelemetry();
  // After the frame render, so a frame that just drew has already carried the
  // strip and this collapses to a no-op rather than refreshing twice over.
  maintainBatteryBanner();
  maintainBleAdvertising();
  delay(5);
}
