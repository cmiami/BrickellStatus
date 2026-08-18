#include <Arduino.h>
#include <NimBLEDevice.h>
#include <heltec-eink-modules.h>

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

#if defined(Vision_Master_E290)
constexpr const PanelWiring &kBuiltFor = kE290;
#else
constexpr const PanelWiring &kBuiltFor = kE213;
#endif

/// Peripheral power, active HIGH and on the same pin on both boards.
constexpr uint8_t kVextPin = 18;

constexpr uint16_t kWidth = kBuiltFor.width;
constexpr uint16_t kHeight = kBuiltFor.height;
constexpr size_t kStride = (kWidth + 7) / 8;
constexpr size_t kPayloadSize = kStride * kHeight;
constexpr size_t kHeaderSize = 18;
constexpr size_t kPacketSize = kHeaderSize + kPayloadSize;
constexpr uint8_t kFlagFullRefresh = 0x01;
constexpr uint32_t kFrameAssemblyTimeoutMs = 1000;

// GATT identifiers intentionally remain compatible with the proven sibling
// InkDock firmware and existing desktop senders. Only the advertised name
// carries the panel, so a host scanning for a board learns which one it found
// before it connects.
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

char bannerLine[96] = "READY";

BaseDisplay *makeDisplay() {
#if defined(Vision_Master_E290)
  return new DEPG0290BNS800();
#elif TENDERS_LOG_PANEL_V11
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
  Serial.println(message);
  if (txCharacteristic != nullptr) {
    txCharacteristic->setValue(message);
    txCharacteristic->notify();
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

void setupBle() {
#if TENDERS_LOG_ENABLE_BLE
  char name[32];
  snprintf(name, sizeof(name), "InkDock %s",
           attached != nullptr ? attached->name : kBuiltFor.name);
  NimBLEDevice::init(name);
  NimBLEDevice::setPower(ESP_PWR_LVL_P3);
  NimBLEServer *server = NimBLEDevice::createServer();
  server->setCallbacks(new ServerCallbacks());
  NimBLEService *service = server->createService(kServiceUuid);
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

  NimBLEAdvertising *advertising = NimBLEDevice::getAdvertising();
  advertising->addServiceUUID(kServiceUuid);
  advertising->setName(name);
  advertising->enableScanResponse(true);
  advertising->start();
#endif
}

void drawWaitingScreen() {
  display->landscape();
  display->clearMemory();
  display->setTextColor(BLACK);
  display->setTextSize(2);
  display->setCursor(14, 18);
  display->print("Tender's Log");
  display->setTextSize(1);
  display->setCursor(15, 53);
  display->print("READY / USB + BLE");
  display->setCursor(15, 72);
  char geometry[32];
  snprintf(geometry, sizeof(geometry), "INK1 / %u x %u", kWidth, kHeight);
  display->print(geometry);
  display->setCursor(15, 91);
  display->print("NO WI-FI / NO LORA");
  display->update();
}

/// The line the host identifies this board by.
///
/// `READY INK1 <w>x<h> <payload> <build> <board> [MISMATCH]`
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
    snprintf(bannerLine, sizeof(bannerLine), "READY INK1 %ux%u %u %s %s",
             kWidth, kHeight, static_cast<unsigned>(kPayloadSize),
             TENDERS_LOG_BUILD_ID, board);
  } else {
    snprintf(bannerLine, sizeof(bannerLine), "READY INK1 0x0 0 %s %s MISMATCH",
             TENDERS_LOG_BUILD_ID, board);
  }
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
  display->clearMemory();
  for (uint16_t y = 0; y < kHeight; ++y) {
    for (uint16_t x = 0; x < kWidth; ++x) {
      const size_t offset = static_cast<size_t>(y) * kStride + x / 8;
      const bool black = (frame[offset] & (0x80u >> (x % 8))) != 0;
      display->drawPixel(x, y, black ? BLACK : WHITE);
    }
  }
  display->update();
  acknowledge("ACK INK1");
}

}  // namespace

void setup() {
  Serial.setRxBufferSize(kPacketSize + 64);
  Serial.begin(115200);
  Serial.setTimeout(50);

  attached = probePanel();
  // Only a positive identification of the *other* board stops this build from
  // driving. A probe that could not tell falls back to the board this image was
  // written for, which is how the firmware behaved before it could ask at all:
  // the probe is allowed to redirect a flash, never to take a working panel out
  // of service because it read an unfamiliar line.
  driving = attached == nullptr || attached == &kBuiltFor;
  composeBanner();
  if (driving) {
    // Only now: the panel this build knows how to talk to is the one attached.
    display = makeDisplay();
    drawWaitingScreen();
  }

  setupBle();
  // The build id lets the host tell whether the device is running the firmware
  // the app ships. Without it a working board can only be reported as "unknown
  // build", never as up to date, so the app would have no basis for offering a
  // flash and no basis for staying quiet.
  Serial.println(bannerLine);
}

void loop() {
  serialDecoder.poll();
  bleDecoder.poll();
  uint8_t chunk[256];
  while (Serial.available() > 0) {
    const size_t count = Serial.readBytes(chunk, sizeof(chunk));
    if (count == 0) break;
    serialDecoder.push(chunk, count);
  }

  renderPendingFrame();
  delay(5);
}
