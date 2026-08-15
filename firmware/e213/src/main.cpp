#include <Arduino.h>
#include <NimBLEDevice.h>
#include <heltec-eink-modules.h>

#include <array>
#include <cstring>

#if TENDERS_LOG_PANEL_V11
EInkDisplay_VisionMasterE213V1_1 display;
#else
EInkDisplay_VisionMasterE213 display;
#endif

namespace {

constexpr uint16_t kWidth = 250;
constexpr uint16_t kHeight = 122;
constexpr size_t kStride = (kWidth + 7) / 8;
constexpr size_t kPayloadSize = kStride * kHeight;
constexpr size_t kHeaderSize = 18;
constexpr size_t kPacketSize = kHeaderSize + kPayloadSize;
constexpr uint8_t kFlagFullRefresh = 0x01;
constexpr uint32_t kFrameAssemblyTimeoutMs = 1000;

// Name and GATT identifiers intentionally remain compatible with the proven
// sibling InkDock firmware and existing desktop senders.
constexpr char kBleName[] = "InkDock E213";
constexpr char kServiceUuid[] = "8b7a0000-4f4b-4a9b-9d6e-1d0c1a2b3c4d";
constexpr char kRxUuid[] = "8b7a0001-4f4b-4a9b-9d6e-1d0c1a2b3c4d";
constexpr char kTxUuid[] = "8b7a0002-4f4b-4a9b-9d6e-1d0c1a2b3c4d";

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

bool submitPacket(const uint8_t *packet, size_t length) {
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
  NimBLEDevice::init(kBleName);
  NimBLEDevice::setPower(ESP_PWR_LVL_P3);
  NimBLEServer *server = NimBLEDevice::createServer();
  server->setCallbacks(new ServerCallbacks());
  NimBLEService *service = server->createService(kServiceUuid);
  NimBLECharacteristic *rx = service->createCharacteristic(
      kRxUuid, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
  txCharacteristic = service->createCharacteristic(
      kTxUuid, NIMBLE_PROPERTY::READ | NIMBLE_PROPERTY::NOTIFY);
  rx->setCallbacks(new RxCallbacks());
  txCharacteristic->setValue("READY");
  service->start();

  NimBLEAdvertising *advertising = NimBLEDevice::getAdvertising();
  advertising->addServiceUUID(kServiceUuid);
  advertising->setName(kBleName);
  advertising->enableScanResponse(true);
  advertising->start();
#endif
}

void drawWaitingScreen() {
  display.landscape();
  display.clearMemory();
  display.setTextColor(BLACK);
  display.setTextSize(2);
  display.setCursor(14, 18);
  display.print("Tender's Log");
  display.setTextSize(1);
  display.setCursor(15, 53);
  display.print("READY / USB + BLE");
  display.setCursor(15, 72);
  display.print("INK1 / 250 x 122");
  display.setCursor(15, 91);
  display.print("NO WI-FI / NO LORA");
  display.update();
}

void renderPendingFrame() {
  if (!framePending) return;

  std::array<uint8_t, kPayloadSize> frame{};
  bool fullRefresh;
  portENTER_CRITICAL(&frameMux);
  std::memcpy(frame.data(), pendingFrame.data(), kPayloadSize);
  fullRefresh = pendingFullRefresh;
  framePending = false;
  portEXIT_CRITICAL(&frameMux);

  if (fullRefresh) {
    display.fastmodeOff();
  } else {
    display.fastmodeOn();
  }
  display.clearMemory();
  for (uint16_t y = 0; y < kHeight; ++y) {
    for (uint16_t x = 0; x < kWidth; ++x) {
      const size_t offset = static_cast<size_t>(y) * kStride + x / 8;
      const bool black = (frame[offset] & (0x80u >> (x % 8))) != 0;
      display.drawPixel(x, y, black ? BLACK : WHITE);
    }
  }
  display.update();
  acknowledge("ACK INK1");
}

}  // namespace

void setup() {
  Serial.setRxBufferSize(kPacketSize + 64);
  Serial.begin(115200);
  Serial.setTimeout(50);
  drawWaitingScreen();
  setupBle();
  // The build id lets the host tell whether the device is running the firmware
  // the app ships. Without it a working board can only be reported as "unknown
  // build", never as up to date, so the app would have no basis for offering a
  // flash and no basis for staying quiet.
  Serial.printf("READY INK1 %ux%u %u %s\n", kWidth, kHeight,
                static_cast<unsigned>(kPayloadSize), TENDERS_LOG_BUILD_ID);
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
