# ESP32-S3 Audio Streamer & Logger Specifications

## 1. Components List
* **Board:** Waveshare ESP32-S3-WROOM-1-N8R8 (8 MB Flash, 8 MB Octal PSRAM)[cite: 1].
* **Microphone:** INMP441 (Digital I2S, omnidirectional)[cite: 1].
* **Storage:** MicroSD Card Module (SPI, native 3.3V)[cite: 1].
* **Display:** 0.91" 128x32 I2C OLED (SSD1306, default address `0x3C`)[cite: 1].
* **Motion Sensor:** AM312 Mini PIR module (Digital out, 3.3V logic).
* **Analog Input:** 10 kΩ 3-pin potentiometer (audio gain/threshold adjustment)[cite: 1].
* **Buttons:** 3 tactile momentary switches (Mode switch, Rec/Stop, Screen toggle).
* **Power:** 3.3V and GND shared across all modules on an 840-tie-point breadboard[cite: 1].

## 2. Hardware Pinout
* **INMP441 (I2S0):**
  * `SCK` (BCLK) -> **GPIO 14**[cite: 1]
  * `WS` (LRCLK) -> **GPIO 15**[cite: 1]
  * `SD` (Data Out) -> **GPIO 16**[cite: 1]
  * `L/R` -> **GND** (Left channel mono)[cite: 1]
* **MicroSD Module (SPI2 / FSPI):**
  * `SCK` -> **GPIO 12**[cite: 1]
  * `MOSI` -> **GPIO 11**[cite: 1]
  * `MISO` -> **GPIO 13**[cite: 1]
  * `CS` -> **GPIO 10** (Pull-up enabled)[cite: 1]
* **0.91" OLED (I2C0):**
  * `SDA` -> **GPIO 8**[cite: 1]
  * `SCL` -> **GPIO 9**[cite: 1]
* **AM312 PIR Sensor:**
  * `OUT` (Center pin) -> **GPIO 3**
* **Potentiometer:**
  * Wiper -> **GPIO 4** (`ADC1_CH3`)[cite: 1]
* **Buttons (Active LOW, internal pull-up):**
  * Button 1 (Mode): **GPIO 5**[cite: 1]
  * Button 2 (Rec/Stop): **GPIO 6**[cite: 1]
  * Button 3 (Display Toggle): **GPIO 7**[cite: 1]
* **Hardware Warning:** Avoid **GPIO 33–37**. They are permanently allocated to the high-speed Octal PSRAM bus; wiring anything to them will corrupt memory and crash the system[cite: 1].

## 3. Implementation Decisions & Architecture
* **Stack:** Rust with standard library support via `esp-idf-svc` (`xtensa-esp32s3-espidf` target)[cite: 1].
* **Dual-Core Workload Partitioning:**
  * **Core 1:** Real-time tasks. INMP441 I2S DMA buffer acquisition, ADC potentiometer polling, button debouncing, and OLED rendering[cite: 1].
  * **Core 0:** Networking, encoding, and storage. Wi-Fi stack, WebSocket server (for live browser streaming), real-time Ogg Opus encoding, and FAT32 SD card file writing[cite: 1].
* **Memory Management:** Audio ring buffers and encoder work areas must be allocated in the 8 MB Octal PSRAM using `sdkconfig.defaults` (`CONFIG_SPIRAM=y`, `CONFIG_SPIRAM_MODE_OCT=y`, `CONFIG_SPIRAM_USE_MALLOC=y`) to absorb SD write stalls (50–250 ms) without dropping audio samples[cite: 1].
* **Audio Capture Configuration:**
  * Sample rate: 16 kHz, Mono (Left channel)[cite: 1].
  * Data width: 24-bit inside a 32-bit slot (`SlotBitWidth::BitWidth32`)[cite: 1].
  * Converted to 16-bit signed PCM frames (20 ms / 320 samples per frame) for streaming and compression[cite: 1].
* **Audio Compression & Storage Format:**
  * **Codec:** Opus configured for fixed-point math and voice mode (`OPUS_APPLICATION_VOIP` or `OPUS_APPLICATION_AUDIO`) at 16–24 kbps VBR (~9 MB/hour)[cite: 1].
  * **Container:** Ogg framing (`.ogg` files) written directly to the FAT32 MicroSD card[cite: 1].
  * **Browser Streaming:** Transmit raw 16-bit linear PCM frames (or pre-encoded Opus packets) via WebSockets[cite: 1].
* **Display Configuration:**
  * Initialize explicitly as $128 \times 32$ pixels (`DisplaySize128x32`) to prevent line-interleaving corruption[cite: 1].
  * Screen toggle button sends software sleep (`0xAE`) and wake (`0xAF`) commands to preserve panel life[cite: 1].
* **Transmission Modes:**
  * Mode A: Live WebSocket binary streaming to browser client[cite: 1].
  * Mode B: Offline continuous Ogg Opus recording to MicroSD card[cite: 1].
  
  
-----

# 1. Install toolchain managers
cargo install espup[cite: 1]
cargo install ldproxy cargo-espflash[cite: 1]

# 2. Install the Xtensa toolchain for ESP32-S3
espup install[cite: 1]

# 3. Source environment variables before compiling (add to ~/.bashrc or run in session)
. ~/export-esp.sh[cite: 1]

# 4. Generate project template
cargo generate esp-rs/esp-idf-template cargo --name esp32s3-audio-node[cite: 1]

# 5. Build and flash
cargo espflash flash --release --monitor /dev/ttyACM0[cite: 1]
