# ESP32-S3 Audio Node - Agent Guide & Operational Context

This document provides essential instructions, hardware constraints, toolchain details, and verification commands to enable agents to work efficiently in this repository from scratch without initial exploration.

---

## 1. Hardware Platform & Architecture

- **Target Board:** Waveshare ESP32-S3-WROOM-1-N8R8
  - **Flash:** 8 MB Quad SPI Flash
  - **PSRAM:** 8 MB Octal PSRAM
- **Dual-Core Workload Split:**
  - **Core 1:** Real-time I2S audio acquisition, ADC polling, button handling, and OLED rendering.
  - **Core 0:** Networking (Wi-Fi, HTTP/streaming server), Ogg Opus audio encoding, and MicroSD card writing.
  - **Memory:** Audio buffers and codec allocations use PSRAM-backed memory to absorb SD write latency.

### Critical Hardware & SDK Constraints
> [!CAUTION]
> - **Reserved PSRAM Bus (GPIO 33-37):** GPIO 33, 34, 35, 36, and 37 are permanently allocated to the Octal PSRAM bus. **NEVER** assign, probe, configure, or read from GPIO 33-37 in software. Doing so corrupts the PSRAM bus and causes an immediate kernel crash.
> - **I2S IRAM Safe Configuration (`CONFIG_I2S_ISR_IRAM_SAFE=n`):** `CONFIG_I2S_ISR_IRAM_SAFE` in `sdkconfig.defaults` MUST remain disabled (`=n`). `esp-idf-hal` places Rust event callbacks in flash rather than IRAM. Enabling this setting causes `i2s_channel_register_event_callback` to fail with `ESP_ERR_INVALID_ARG` and breaks microphone initialization.

### Fixed Pin Mapping
| Peripheral | Signal | GPIO | Notes |
| :--- | :--- | :--- | :--- |
| **INMP441 Mic** | SCK / BCLK | **GPIO 14** | I2S0 clock |
| | WS / LRCK | **GPIO 15** | I2S0 word select |
| | SD / DATA | **GPIO 16** | I2S0 serial data (L/R pin tied to GND for mono) |
| **MicroSD SPI** | SCK | **GPIO 12** | SPI bus |
| | MOSI | **GPIO 11** | SPI bus |
| | MISO | **GPIO 13** | SPI bus |
| | CS | **GPIO 10** | Chip select (active-low) |
| **OLED Display** | SDA | **GPIO 8** | I2C0 data (SSD1306, 128x32, address `0x3C`/`0x3D`) |
| | SCL | **GPIO 9** | I2C0 clock |
| **PIR Sensor** | OUT | **GPIO 3** | Digital motion input (internal pull-down, active-high) |
| **Potentiometer**| ADC | **GPIO 4** | ADC1_CH3 analog input for gain / level control |
| **Button 1 (Mode)** | IN | **GPIO 5** | Active-low, internal pull-up. Short press: cycle mode. Long press: screen power toggle. |
| **Button 2 (Rec)** | IN | **GPIO 6** | Active-low, internal pull-up. |
| **Button 3 (Other)**| IN | **GPIO 7** | Active-low, internal pull-up. |

---

## 2. Toolchain & Verification Commands

The repository targets `xtensa-esp32s3-espidf` using the Espressif Rust toolchain (`esp`).

### Fast Compilation & Syntax Check
```bash
# Debug profile check
cargo check

# Release profile check (recommended for size and optimization verification)
cargo check --release
```

### Unit Testing & Verification
```bash
# Compile binary and unit tests without executing runner
cargo test --no-run
```
> [!IMPORTANT]
> - **DO NOT** execute plain `cargo test`. `Cargo.toml` sets `[[bin]] harness = false`, and `.cargo/config.toml` configures a hardware runner (`espflash flash --monitor`). Running `cargo test` without a physical test runner fails with `No such file or directory (os error 2)`.
> - **DO NOT** target host `cargo test --target x86_64-unknown-linux-gnu`. `esp-idf-sys` only supports ESP targets and will fail custom build scripts on host x86.
> - Always verify unit tests and code compilation using `cargo test --no-run`.

### Flashing Hardware & Serial Monitoring
When requested to flash or verify on board:
```bash
# Flash release binary to board
cargo espflash flash --release --target xtensa-esp32s3-espidf --port /dev/ttyACM0

# Monitor serial console at 115200 baud
espflash monitor --port /dev/ttyACM0
```

---

## 3. Codebase Architecture & Module Boundaries

The code follows clean modular boundaries established in `src/`:

```
src/
├── main.rs         # Application entry point
├── app.rs          # Thin application orchestration and main polling loop
├── config.rs       # Board baseline definitions, pin validation, runtime architecture
├── modes.rs        # DeviceMode enum (Status, Live, Sd) & ModeButtonController state machine
├── display.rs      # OledDisplay manager, power toggles, and modular per-mode renderers
├── status.rs       # SystemStatus aggregation for sensors and network state
├── sd.rs           # MicroSD card lifecycle, probing, and FATFS file system mounting
├── network.rs      # Local Wi-Fi connection and credentials loader
├── potentiometer.rs# ADC1_CH3 potentiometer driver & linear software gain mapping
└── audio/
    ├── mod.rs      # Audio module root
    ├── frame.rs    # AudioFrame data structures
    ├── mic.rs      # INMP441 Microphone driver & signal detection
    └── stream.rs   # LiveAudioStream capture logic
```

### Key Design Conventions
1. **Screen Management (`src/display.rs`):**
   - Use `OledDisplay::init(i2c)` to initialize the OLED display.
   - Use `display.is_on()` and `display.set_power(bool)` for display sleep/wake.
   - Add/edit per-mode screens directly in `render_status_screen`, `render_live_screen`, or `render_sd_screen`. `App::run` delegates rendering via `display.render(mode, connection, status)`.
2. **Button Interactions (`src/modes.rs` & `src/app.rs`):**
   - **Button 1 (GPIO 5):** Managed by `ModeButtonController`. Short press (< 800ms): `ModeButtonAction::CycleMode`. Long press (>= 800ms): `ModeButtonAction::TurnScreenOff`. Press when screen is off: `ModeButtonAction::TurnScreenOn` (wakes display without cycling mode).
   - **Buttons 2 & 3 (GPIO 6 & 7):** Configured with internal pull-ups (`Pull::Up`), active-low. Polled in loop with edge-detection logging.
3. **Microphone Acquisition (`src/audio/mic.rs`):**
   - Use `Microphone::new(i2s0, pins.gpio14, pins.gpio15, pins.gpio16)` to initialize the INMP441 (Philips standard, 16 kHz, 32-bit slot).
   - Must call `driver.rx_enable()?` on driver initialization.
   - Use `read_samples(&mut buf, timeout_ticks)` for live acquisition and `probe_signal()` / `detect_signal(&buf)` for acoustic activity checks.
4. **Sensor Status Aggregation (`src/status.rs`):**
   - Use `SystemStatus::from_runtime_with_sensors(wifi, mic, pir, sd_card, gain_percent)` to feed real live sensor states and potentiometer gain to STATUS_MODE OLED rendering (`g:<N>%`).
5. **SD Card Management (`src/sd.rs`):**
   - Do **NOT** call `Peripherals::take()` inside helper functions. Pass acquired pins/peripherals down from `App::run`.
6. **Live Audio Capture & Ring Buffer (`src/audio/stream.rs` & `src/audio/mic.rs`):**
   - Real-time I2S audio capture runs in a dedicated worker thread pinned to **Core 1** via `ThreadSpawnConfiguration` (`Core::Core1`).
   - Use `SharedAudioBuffer` (PSRAM-backed ring buffer) to decouple real-time capture from Core 0 network/recording tasks. Consumers track progress with monotonic sequence IDs without blocking the capture thread.
   - `convert_i2s_bytes_to_pcm16_with_gain(raw, gain: f32)` scales 32-bit INMP441 samples to 16-bit PCM linearly (`gain <= 0.0` outputs all zeros for digital silence).
   - Use `audio_buffer.set_gain(gain, percent)` / `audio_buffer.current_gain()` / `audio_buffer.current_gain_percent()` for non-blocking lock-free atomic gain sharing between Core 1 and Core 0. Query `audio_buffer.is_signal_present()` for sensor status checks to prevent I2S hardware read contention.
7. **HTTP Server & Web Assets (`web/index.html` & `src/audio/stream.rs`):**
   - Browser UI templates must live in `web/index.html` (embedded via `include_str!("../../web/index.html")` using `{{ENDPOINT}}` substitution) to keep web assets and Rust code cleanly decoupled.
   - Live audio endpoint `/stream.wav` streams chunked 16-bit PCM prefixed by a 44-byte WAV header (`create_wav_header`) with `0x7fff_ffff` streaming chunk size.
8. **Wi-Fi Driver Persistence (`src/network.rs`):**
   - `BlockingWifi` / `EspWifi` shuts down the radio on drop. Keep the driver permanently active using `std::mem::forget(wifi)` in `connect_with_modem`.
9. **Potentiometer & ADC Gain Control (`src/potentiometer.rs`):**
   - Use `Potentiometer::new(adc1, pins.gpio4)` to initialize the 12-bit oneshot ADC driver on `ADC1` (`ADCCH3<ADCU1>`, attenuation `DB_12`).
   - Poll `pot.read_gain()` inside the Core 1 audio capture loop (each 20ms frame). Raw ADC values map linearly: deadband (`raw <= 40`) produces `0.0` (silence), full scale (`4095`) produces `4.0x` max gain, and intermediate values scale proportionally.

---

## 4. OpenSpec Workflow & Task Progression

The project specification and tasks are tracked under `openspec/changes/esp32s3-audio-node/`.

### Commands
```bash
# Check change status and artifact completeness
openspec status --change "esp32s3-audio-node" --json

# Validate consistency between proposal, design, specs, and tasks
openspec validate esp32s3-audio-node --json

# View remaining tasks and apply instructions
openspec instructions apply --change "esp32s3-audio-node" --json
```

### Workflow Rules
- Check `openspec/changes/esp32s3-audio-node/tasks.md` for task numbering and checklist status.
- When working on `/opsx-apply task X.Y`:
  1. Implement changes cleanly and minimally according to the spec.
  2. Verify compilation with `cargo test --no-run` and `cargo check --release`.
  3. Mark task complete in `tasks.md` (`- [x] X.Y ...`).
- When updating plans or requirements, ensure `proposal.md`, `design.md`, `specs/audio-streaming/spec.md`, and `tasks.md` remain coherent, then run `openspec validate esp32s3-audio-node --json`.
