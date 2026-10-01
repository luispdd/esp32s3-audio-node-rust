# ESP32-S3 Audio Node - Agent Guide & Operational Context

This document provides essential instructions, hardware constraints, toolchain details, and verification commands to enable agents to work efficiently in this repository from scratch without initial exploration.

---

## 1. Hardware Platform & Architecture

- **Target Board:** Waveshare ESP32-S3-WROOM-1-N8R8
  - **Flash:** 8 MB Quad SPI Flash
  - **PSRAM:** 8 MB Octal PSRAM
- **Dual-Core Workload Split:**
  - **Core 1:** Real-time I2S audio acquisition, ADC polling, button handling, and OLED rendering.
  - **Core 0:** Networking (Wi-Fi, HTTP/streaming server), standard WAV audio recording, and MicroSD card writing.
  - **Memory:** Audio buffers use PSRAM-backed memory to absorb SD write latency.

### Critical Hardware & SDK Constraints
> [!CAUTION]
> - **Reserved PSRAM Bus (GPIO 33-37):** GPIO 33, 34, 35, 36, and 37 are permanently allocated to the Octal PSRAM bus. **NEVER** assign, probe, configure, or read from GPIO 33-37 in software. Doing so corrupts the PSRAM bus and causes an immediate kernel crash.
> - **I2S IRAM Safe Configuration (`CONFIG_I2S_ISR_IRAM_SAFE=n`):** `CONFIG_I2S_ISR_IRAM_SAFE` in `sdkconfig.defaults` MUST remain disabled (`=n`). `esp-idf-hal` places Rust event callbacks in flash rather than IRAM. Enabling this setting causes `i2s_channel_register_event_callback` to fail with `ESP_ERR_INVALID_ARG` and breaks microphone initialization.
> - **Opus-rs Xtensa LLVM Instruction Selection:** The Xtensa LLVM backend contains an instruction-selection bug on certain constants (`Constant<-32768>` / `Constant<8192>`) during full release optimization. `Cargo.toml` MUST retain `[profile.release.package.opus-rs]` with `opt-level = "z"`, `debug-assertions = true`, and `overflow-checks = true` to prevent codegen crashes.

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
| **Button 2 (Rec)** | IN | **GPIO 7** | Active-low, internal pull-up. Hardware board mapping for recording toggle. |
| **Button 3 (Other)**| IN | **GPIO 6** | Active-low, internal pull-up. Hardware board mapping for recording cancel / discard. |
| **RGB LED (WS2812)**| DATA | **GPIO 38** | Onboard addressable LED (RMT driver). Blue during boot, off after boot. |

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
# Flash release binary to board (serial port typically /dev/ttyACM0)
cargo espflash flash --release --target xtensa-esp32s3-espidf --port /dev/ttyACM0

# Monitor serial console via cargo-espflash (115200 baud)
cargo espflash monitor --port /dev/ttyACM0

# Quick 3-second serial log inspection (non-blocking)
timeout 3 cargo espflash monitor --port /dev/ttyACM0

# Non-blocking software reboot into app (avoids ROM download mode)
python3 -c "import serial, time; s = serial.Serial('/dev/ttyACM0', 115200, timeout=1); s.dtr = False; s.rts = True; time.sleep(0.1); s.rts = False; s.close()"
```
> [!NOTE]
> - `espflash` is installed as a Cargo subcommand (`cargo-espflash` at `~/.cargo/bin/cargo-espflash`). Always invoke it via `cargo espflash ...` or the full binary path.
> - **DTR/RTS Bootloader Trap:** On the ESP32-S3 USB-JTAG/CDC interface, serial monitors that toggle DTR and RTS simultaneously trigger the chip's ROM download bootloader (`DOWNLOAD(USB/UART0)`). To perform a clean reset into the flashed user application partition without entering download mode, assert `RTS=True` (EN low) while keeping `DTR=False` (BOOT high), sleep 100ms, then release `RTS=False` (EN high).
> - When testing device web endpoints over LAN, use `curl -m 2 http://<DEVICE_IP>/status` or `curl -m 2 http://<DEVICE_IP>/api/recordings`.

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
├── led.rs          # Onboard WS2812 RGB LED driver (RMT), boot indicator
├── time.rs         # NTP client & UTC datetime formatting for recordings (YYYYMMDD_HHMMSS)
└── audio/
    ├── mod.rs      # Audio module root
    ├── frame.rs    # AudioFrame data structures
    ├── mic.rs      # INMP441 Microphone driver & signal detection
    ├── playback.rs # On-device WAV playback worker & lifecycle controller
    ├── recorder.rs # Standard WAV audio recording worker and lifecycle controller
    └── stream.rs   # LiveAudioStream capture logic & HTTP streaming server
```

### Key Design Conventions & Operational Findings
1. **Screen Management (`src/display.rs`):**
   - Use `OledDisplay::init(i2c)` to initialize the OLED display.
   - Use `display.is_on()` and `display.set_power(bool)` for display sleep/wake.
   - Add/edit per-mode screens directly in `render_status_screen`, `render_live_screen`, or `render_sd_screen`. `App::run` delegates rendering via `display.render(mode, connection, status)`.
2. **Button Interactions (`src/modes.rs` & `src/app.rs`):**
   - **Button 1 (GPIO 5):** Managed by `ModeButtonController`. Short press (< 800ms): `ModeButtonAction::CycleMode`. Long press (>= 800ms): `ModeButtonAction::TurnScreenOff`. Press when screen is off: `ModeButtonAction::TurnScreenOn` (wakes display without cycling mode).
   - **Button 2 (GPIO 7 physically):** Configured with internal pull-up (`Pull::Up`), active-low. In `SD_MODE`, index 0 is `[Record new]` (default on entering `SD_MODE`). When cursor is on `[Record new]`, pressing Button 2 starts recording; when cursor is over an existing file, pressing Button 2 toggles playback. While recording is in progress, pressing Button 2 stops and saves the recording. Long press (>= 800ms) also starts recording.
   - **Button 3 (GPIO 6 physically):** Configured with internal pull-up (`Pull::Up`), active-low. In `SD_MODE`, short press advances cursor through `[Record new]` and existing recordings in reverse chronological order (wrapping around). While recording is in progress, pressing Button 3 cancels and discards the file.
   - **Dual-Button Chord (B2 + B3):** In `SD_MODE`, simultaneously holding Button 2 and Button 3 for >= 1000ms deletes the currently selected recording (ignored when cursor is on `[Record new]`).
   - **Button Release Suppression Rule:** When leading-edge actions (`StopRecording`, `CancelRecording`, `TogglePlayback`) transition device states while the button is still physically depressed, the controller MUST set release suppression flags (`b2_suppress_release` / `b3_suppress_release`). This prevents the subsequent physical release of the button (100-250ms later) from being misidentified as a new short-press gesture in the resulting idle state.
3. **Microphone Acquisition (`src/audio/mic.rs`):**
   - Use `Microphone::new(i2s0, pins.gpio14, pins.gpio15, pins.gpio16)` to initialize the INMP441 (Philips standard, 16 kHz, 32-bit slot).
   - Must call `driver.rx_enable()?` on driver initialization.
   - Use `read_samples(&mut buf, timeout_ticks)` for live acquisition and `probe_signal()` / `detect_signal(&buf)` for acoustic activity checks.
4. **Sensor Status Aggregation (`src/status.rs`):**
   - Use `SystemStatus::from_runtime_with_sensors(wifi, mic, pir, sd_card, gain_percent)` to feed real live sensor states and potentiometer gain to STATUS_MODE OLED rendering (`g:<N>%`).
5. **MicroSD Card & FATFS SPI Stack Safety (`src/sd.rs` & `src/audio/recorder.rs`):**
   - Do **NOT** call `Peripherals::take()` inside helper functions. Pass acquired pins/peripherals down from `App::run`.
   - MicroSD operates on SPI3 with FATFS mounted at `/sdcard`. Recordings belong in `/sdcard/audio`.
   - **Stack & Memory Rule for SD File IO:** Any thread executing FATFS/SD operations (e.g. `File::create`, `write`) must have its stack allocated in **internal SRAM** (`< 16 KB`) with 32-bit alignment capabilities: `enum_set!(MallocCap::Internal | MallocCap::Cap32bit | MallocCap::Cap8bit)`. Allocating stack in PSRAM or omitting `Cap32bit` triggers MMU bus contention and `LoadStoreAlignment` hardware panics on Xtensa.
   - **FATFS File Open & Seek Permissions:** Files opened for recording MUST use `OpenOptions::new().read(true).write(true).create(true).truncate(true)` so FATFS allows seek operations. When finalizing the WAV container, `BufWriter::flush()` MUST be called prior to seeking to offsets 4 and 40 to ensure all audio samples are written to disk before header length fields are overwritten.
6. **Live Audio Capture & Ring Buffer (`src/audio/stream.rs` & `src/audio/mic.rs`):**
   - Real-time I2S audio capture runs in a dedicated worker thread pinned to **Core 1** via `ThreadSpawnConfiguration` (`Core::Core1`).
   - Use `SharedAudioBuffer` (PSRAM-backed ring buffer) to decouple real-time capture from Core 0 network/recording tasks. Consumers track progress with monotonic sequence IDs without blocking the capture thread.
   - `convert_i2s_bytes_to_pcm16_with_gain(raw, gain: f32)` scales 32-bit INMP441 samples to 16-bit PCM linearly (`gain <= 0.0` outputs all zeros for digital silence).
   - Use `audio_buffer.set_gain(gain, percent)` / `audio_buffer.current_gain()` / `audio_buffer.current_gain_percent()` for non-blocking lock-free atomic gain sharing between Core 1 and Core 0. Query `audio_buffer.is_signal_present()` for sensor status checks to prevent I2S hardware read contention.
7. **HTTP Server & Web Client Efficiency (`web/index.html` & `src/audio/stream.rs`):**
   - Browser UI templates must live in `web/index.html` (embedded via `include_str!("../../web/index.html")` using `{{ENDPOINT}}` substitution) to keep web assets and Rust code cleanly decoupled.
   - Live audio endpoint `/stream.wav` streams chunked 16-bit PCM prefixed by a 44-byte WAV header (`create_wav_header`) with `0x7fff_ffff` streaming chunk size.
   - Stored recording endpoints: `GET /api/recordings` (JSON listing with date and size), `GET /recordings/{filename}` (streams `audio/wav`), and `DELETE /api/recordings?filename=...` (deletes file from SD).
   - **Energy-Efficiency Rule:** Avoid automatic high-frequency polling loops (e.g. `setInterval(pollStatus, 1500)`) in the web client. Battery and low-power IoT operation requires pulling status on page load and on explicit user interaction via a **Refresh** button.
8. **Wi-Fi Driver Persistence (`src/network.rs`):**
   - `BlockingWifi` / `EspWifi` shuts down the radio on drop. Keep the driver permanently active using `std::mem::forget(wifi)` in `connect_with_modem`.
9. **Potentiometer & ADC Gain Control (`src/potentiometer.rs`):**
   - Use `Potentiometer::new(adc1, pins.gpio4)` to initialize the 12-bit oneshot ADC driver on `ADC1` (`ADCCH3<ADCU1>`, attenuation `DB_12`).
   - Poll `pot.read_gain()` inside the Core 1 audio capture loop (each 20ms frame). Raw ADC values map linearly: deadband (`raw <= 40`) produces `0.0` (silence), full scale (`4095`) produces `4.0x` max gain, and intermediate values scale proportionally.
10. **Onboard RGB LED (`src/led.rs`):**
    - The onboard WS2812 RGB LED is connected to **GPIO 38** and driven via the RMT peripheral using `RgbLed::new(pins.gpio38)`.
    - It is set to Blue (`set_booting()`) as soon as `App::run` begins, and turned off (`turn_off()`) once all subsystems (Wi-Fi, I2C, SD, Audio, HTTP servers) have finished initializing before entering the main polling loop.
11. **NTP Time Synchronization (`src/time.rs`):**
    - Initialized via `NtpClient::init()` using `EspSntp` in `Poll` operating mode against `pool.ntp.org`.
    - `UtcDateTime::now()` returns UTC calendar components and generates recording filenames: `YYYYMMDD_HHMMSS.wav`.
12. **Audio Recording Architecture (Standard WAV):**
    - Recordings are stored in standard 16 kHz 16-bit mono RIFF WAV format (`/sdcard/audio/YYYYMMDD_HHMMSS.wav`).
    - Standard WAV avoids compression memory overhead, eliminates FreeRTOS thread stack overflows, avoids Xtensa LLVM codegen bugs associated with pure-Rust Opus, and provides out-of-the-box browser playback (`audio/wav`).
13. **Audio Playback Architecture (`src/audio/playback.rs`):**
    - Managed by `PlaybackController`. Spawns a background worker pinned to **Core 0** that reads 16 kHz 16-bit mono PCM samples from the selected file and feeds them into `SharedAudioBuffer`.
    - While playback is active, Core 1 audio capture mutes microphone sample forwarding to `SharedAudioBuffer` so listeners on `/stream.wav` and the web visualizer hear and see the recording playback without acoustic feedback or mic contention.
    - OLED screen in `SD_MODE` displays `SD_MODE [PLAY]`, current/total duration (`MM:SS / MM:SS`), and the filename. Stopping playback or EOF restores live mic capture and returns to the file browser.
14. **File Ordering & Screen Synchronization (`src/sd.rs` & `src/display.rs`):**
    - Recordings follow chronological filenames `YYYYMMDD_HHMMSS.wav`. Alphabetical sorting places newest files at the end; therefore, both `inspect_audio_folder` (OLED rendering) and `list_audio_files` (web API & navigation) MUST reverse the sort (`files.sort(); files.reverse()`) to ensure index 0 represents the newest file.
    - `SD_MODE` prefixes the list with `[Record new]` at index 0. Existing recordings map to `files[selected_index - 1]`.
15. **Synchronous Worker Finalization (`src/audio/recorder.rs`):**
    - `RecordingController::stop()` and `cancel()` must block with a bounded loop (`while self.is_recording() && ...`) to guarantee the Core 0 worker thread finishes flushing, updates WAV headers, and closes file handles before the caller refreshes directory metadata (`card.inspect()`) or updates display state.
16. **PIR Mode & Motion-Triggered Recording Lifecycle (`src/modes.rs`, `src/app.rs`):**
    - `PIR_MODE` operates as the 4th device mode in cyclic sequence: `STATUS_MODE` -> `LIVE_MODE` -> `SD_MODE` -> `PIR_MODE` -> `STATUS_MODE`.
    - Managed by `PirButtonController`:
      - Short press on Button 2 (GPIO 7) initiates a 10-second arming delay (`PirOperationalState::Arming`), ignoring motion.
      - After 10 seconds elapse, transitions to `PirOperationalState::Armed` (monitoring active).
      - Motion detected by the PIR sensor (GPIO 3, active-high) while armed immediately triggers standard WAV recording to `/sdcard/audio/YYYYMMDD_HHMMSS.wav`.
      - Recording is maintained for 20 seconds (`DEFAULT_RECORDING_DURATION`) from the latest motion event, dynamically resetting the 20-second countdown each time new movement is detected by the PIR sensor.
      - Once 20 seconds elapse with no motion, cleanly finalizes the WAV recording via `recording_controller.stop()` and automatically returns to `PirOperationalState::Armed` (ready for the next motion event).
      - Short press on Button 2 while arming, armed, or recording immediately disarms the controller back to `Idle` (and stops/finalizes any active recording).
      - Short press on Button 1 (mode switch) disarms the controller and finalizes recording.
      - Long press on Button 1 toggles OLED display sleep/wake while motion monitoring and recording continue running unaffected in the background.
17. **Buffer Sizing, Pre-roll & SD Read Tuning (`src/audio/stream.rs`):**
    - `DEFAULT_BUFFER_CAPACITY = 150` frames (150 frames * 20ms = 3,000ms = 3 seconds) in `SharedAudioBuffer`. This absorbs MicroSD cluster allocation latency and SPI bus contention without audio drops.
    - Live audio streaming pre-roll: `/stream.wav` client connection initializes `last_seq = stream_buffer.current_seq().saturating_sub(10)` (10 frames = 200ms pre-roll) to prime client audio buffers and prevent initial buffer underrun clicks while maintaining low ~200ms latency.
    - SD file streaming chunk size: `/recordings/{filename}` uses a 4,096-byte chunk buffer (`vec![0u8; 4096]`) instead of 1,024 bytes, reducing filesystem read overhead and improving HTTP streaming throughput over Wi-Fi.
18. **Atomic Cross-Core State Synchronization for Web & REST APIs (`src/audio/stream.rs`):**
    - Real-time device state (device mode, PIR state, arming countdown, motion sensor state, recording countdown) is mirrored to `SharedAudioBuffer` via lock-free atomic variables (`AtomicU8`, `AtomicBool`).
    - Core 1 updates them without blocking or mutex contention on each loop tick; Core 0 HTTP handlers (`GET /status`, `GET /api/pir`) read snapshots instantaneously.
    - Dedicated REST endpoint `GET /api/pir` and status fields in `GET /status` reflect `device_mode`, `pir_state`, `pir_armed`, `pir_arming_countdown`, `pir_motion_detected`, and `pir_recording_remaining_secs`.

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

---

## 5. Troubleshooting & Known Pitfalls

### Cross-Core FreeRTOS File I/O & SD Bus Contention
- **Problem / Symptom:** Calling FATFS operations (e.g. creating/closing files, seeking, writing) or modifying hardware state directly inside Core 0 HTTP request handlers causes random FreeRTOS task starvation, SPI timeouts, or `LoadStoreAlignment` hardware panics on Xtensa.
- **Root Cause:** Core 0 HTTP handler threads run in lwIP/FreeRTOS network task context with stacks not guaranteed to meet FATFS 32-bit alignment constraints. Concurrently executing SPI3 operations while Core 1 audio loops are active corrupts hardware state.
- **Solution:** Keep Core 0 HTTP request handlers completely non-blocking. Dispatch commands to Core 1 via lock-free atomic variables (`AtomicU8` using `swap(0, Ordering::SeqCst)`), and allow Core 1 to execute filesystem mutations and controller transitions synchronously within `App::run`.
- **Verification:** Run `cargo test --no-run` to verify compilation, and verify non-blocking command dispatch via HTTP endpoints.

### ESP-IDF HTTP Server Route Matching & Trailing Wildcard (`*`)
- **Problem / Symptom:** Requests to registered endpoints return HTTP 404 or match incorrect handlers when query parameters (e.g. `?value=...`) or trailing slashes are included.
- **Root Cause:** The `esp-idf-svc` HTTP server route registration matches URIs strictly. A route registered without a trailing `*` will fail to match if query parameters are appended.
- **Solution:** Always register HTTP REST routes with a trailing asterisk (e.g. `/api/endpoint*`), explicitly verify `request.method()` inside the handler, and supply preflight `Method::Options` handlers for browser CORS.
- **Verification:** Test endpoints using `curl -i -X POST http://<DEVICE_IP>/api/endpoint?value=10` and `curl -i -X OPTIONS http://<DEVICE_IP>/api/endpoint`.

### Web Client Slider Overwrites During Status Polling
- **Problem / Symptom:** Interactive range sliders (gain, thresholds) flicker or jump back to old values while the user is actively dragging them.
- **Root Cause:** Asynchronous responses from `/status` polling overwrite `.value` on the slider element while touch or mouse dragging is in progress.
- **Solution:** Track active dragging state (`isDragging = true` on `mousedown`/`touchstart`, `false` on `mouseup`/`touchend`). In status update callbacks, skip updating the slider value while active dragging is true.
- **Verification:** Drag range sliders in the browser and verify smooth adjustment without value reset.

