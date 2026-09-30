## Context

The project targets a Waveshare ESP32-S3-WROOM-1-N8R8 board with 8 MB Flash and 8 MB Octal PSRAM. The firmware is implemented in Rust using the ESP-IDF stack, and the design must support real-time audio capture, browser playback, and local recording without drifting from the original hardware layout described in the project spec.

The hardware baseline is fixed as follows:
- INMP441 microphone: SCK on GPIO 14, WS on GPIO 15, and SD on GPIO 16; L/R tied to GND for mono capture.
- MicroSD module: SCK on GPIO 12, MOSI on GPIO 11, MISO on GPIO 13, and CS on GPIO 10.
- OLED display: SDA on GPIO 8 and SCL on GPIO 9.
- PIR sensor: OUT on GPIO 3.
- Potentiometer ADC: GPIO 4 (ADC1_CH3).
- Buttons: GPIO 5, 6, and 7 for mode, rec/stop, and display toggle.
- Reserved hardware constraint: avoid GPIO 33-37 because they are allocated to the PSRAM bus and can crash the system if used.

The original architecture also defines the workload split:
- Core 1 handles real-time I2S capture, ADC polling, button debouncing, and OLED rendering.
- Core 0 handles Wi-Fi, streaming, codec work, and SD writing.
- PSRAM-backed buffers are required to absorb SD write stalls without dropping audio samples.

## Goals / Non-Goals

**Goals:**
- Provide a reliable live audio path from the microphone to a browser over Wi-Fi.
- Synchronize system time with NTP internet time servers to provide accurate UTC wall-clock timestamps for audio file logging.
- Record microphone audio locally in standard WAV format under `/audio/YYYYMMDD_HHMMSS.wav` on the SD card with robust start, stop, and cancel controls.
- Provide on-device file navigation, playback, and safe two-button deletion in `SD_MODE`.
- Expose stored recordings through a browser-accessible listing, native browser playback, and remote deletion path.
- Keep the device self-contained and usable from a browser without external infrastructure.
- Preserve the original hardware and dual-core constraints defined in the initial project specification.

**Non-Goals:**
- Full cloud or remote streaming services.
- Complex multi-user browser applications.
- Extensive UI polish beyond the minimal browser page needed for playback.
- Broad feature expansion beyond the audio pipeline and control flow defined in this change.

## Decisions

### 1. Deliver the full project in a phased implementation order
The project remains a full-feature audio streamer/logger, but it will be implemented in a practical sequence:
1. Live audio streaming to browser
2. SD-card recording in standard WAV format
3. Playback of stored recordings to browser

This keeps the product complete in scope while reducing risk and validating the critical audio path early.

**Alternative considered:** build all three features in parallel. Rejected because the live streaming path is the highest-risk foundation and should be validated before storage and browser playback features are layered on.

**Alternative considered:** build all three features in parallel. Rejected because the live streaming path is the highest-risk foundation and should be validated before storage and browser playback features are layered on.

### 6. Introduce a device mode system before the audio pipeline
Before the audio pipeline is wired up, the firmware goes through a dedicated system bootstrap phase that establishes the interactive skeleton of the device:
- Three device modes are defined: STATUS_MODE, LIVE_MODE, and SD_MODE. Button 1 (GPIO 5) cycles through them on a short press. A long press on Button 1 turns the OLED screen off; subsequent presses turn the screen back on and restore the active mode view.
- Each mode drives a distinct OLED display layout. LIVE_MODE shows the title and the DHCP IP. SD_MODE verifies the SD card and eventually shows the audio file list. STATUS_MODE shows live sensor readings: Wi-Fi, microphone signal, PIR activity, and SD availability.
- Screen management is extracted into a dedicated `src/display.rs` module with an extensible, mode-driven API so that future mode additions and richer content do not require changes to the core application loop.
- SD card probing and lifecycle management are extracted into a dedicated `src/sd.rs` module. This fixes a startup-ordering bug where the probe function attempted to re-acquire the hardware peripherals after the main application had already taken them.
- Buttons 2 and 3 (GPIO 6 and 7) are initialized and log to the console as stubs, ready for their future roles.

This phase produces a functional, interactive device that can be verified on the board before any real audio pipeline work begins.

**Alternative considered:** skipping the mode system and going directly to audio capture. Rejected because the mode system provides a verification baseline on real hardware for display, Wi-Fi, SD, and sensor wiring before the more complex audio pipeline is layered on top.

### 7. Refactor the source structure before adding behavioral features
Before any additional features are layered onto the firmware, the source tree must be restructured to follow Rust module best practices. `src/app.rs` has grown to 460+ lines and mixes hardware types, domain state, display rendering, SD probing, audio types, and application orchestration — a clear violation of the single-responsibility principle.

The target layout separates concerns into focused modules:
- `src/modes.rs` — `DeviceMode` and its cycling/naming logic
- `src/status.rs` — `SystemStatus` and sensor aggregation
- `src/display.rs` — OLED probe, initialization, and mode-driven rendering
- `src/sd.rs` — SD card lifecycle, probing, and the FATFS mount abstraction
- `src/audio/frame.rs` — `AudioFrame` data type
- `src/audio/stream.rs` — `LiveAudioStream` capture interface
- `src/app.rs` — thin orchestration: wires modules and owns the main loop
- `src/main.rs` — entry point, unchanged

This refactoring produces no behavioral change and does not touch the hardware at runtime. All existing tests are relocated to their home modules and must continue to pass. The restructuring is a prerequisite for tasks 2.5 through 2.11 to remain tractable as the codebase grows.

**Alternative considered:** continuing to grow `src/app.rs` until it is too large to navigate. Rejected because the compound growth of display, SD, audio, and networking concerns will make the file unmaintainable before the audio pipeline phase is complete.

### 2. Keep browser interaction minimal and local
The browser experience will be minimal but functional. The device may serve a tiny HTML page or provide a direct stream endpoint if that is the simplest way to get live audio and recorded playback working without external dependencies.

**Alternative considered:** building a full-feature web application front end. Rejected because the core requirement is browser playback and device self-containment, not a polished app experience.

### 3. Use standard 16 kHz 16-bit mono WAV for offline recording with UTC timestamped filenames
The system will record audio as standard WAV files (RIFF format) to the SD card.
- Recordings are created under the `/audio` directory on the FATFS-mounted SD card.
- Files are named in the format `YYYYMMDD_HHMMSS.wav` using the UTC wall-clock time at the instant recording begins (obtained from the NTP-synchronized system clock).
- Core 1 streams audio frames into the PSRAM ring buffer, while a recording worker on Core 0 pulls frames, writes linear 16-bit PCM samples into the WAV data chunk, and finalizes the 44-byte WAV header upon completion.
- WAV recording avoids heavy codec memory allocations and stack bloat on FreeRTOS threads, ensuring robust, low-latency writes to the SD card without memory exhaustion or stack overflows.

**Alternative considered:** Ogg Opus recording. Evaluated and rejected due to Pure-Rust `opus-rs` requiring stack allocations exceeding available FreeRTOS thread limits in internal SRAM on ESP32-S3, which causes stack overflows, MMU bus contention, and runtime crashes. Standard WAV format provides guaranteed hardware stability, low CPU consumption, and native browser compatibility (`audio/wav`).

### 4. Respect the original audio pipeline configuration
The design will keep the capture configuration aligned with the initial spec: 16 kHz mono audio, 24-bit data packed into a 32-bit slot, converted into 16-bit PCM frames for streaming and storage.

**Alternative considered:** deviating from the original sample setup. Rejected because the project’s hardware and architecture assumptions depend on that configuration for stability and audio quality.

### 5. Keep Wi-Fi credentials outside source control
The firmware will read credentials from a local, git-ignored file to avoid committing secrets while preserving a simple deployment model for local development and field use.

**Alternative considered:** embedding credentials in source. Rejected because source control safety and operational cleanliness are more important than convenience.

### 8. Use the potentiometer (GPIO 4 / ADC1_CH3) as a real-time software gain control
The potentiometer wiper is polled periodically on Core 1 alongside the other real-time tasks (I2S capture, button debouncing). Its raw ADC reading is mapped linearly to a floating-point gain multiplier: 0 at the minimum ADC value (wiper fully counter-clockwise → silence) and a configurable maximum at full scale (wiper fully clockwise → loudest). The multiplier is applied inside `convert_i2s_bytes_to_pcm16_with_gain`, which already accepts a gain parameter, so no new audio path is needed. The current gain level derived from the ADC reading is reflected on the STATUS_MODE OLED screen alongside the other sensor readings.

**Alternative considered:** hardware gain via a dedicated amplifier circuit. Rejected because the INMP441 has no programmable hardware gain register, and introducing an external circuit adds complexity without a meaningful quality benefit at the target sample rate and bit depth.

**Alternative considered:** exposing gain as a network-settable parameter rather than a physical control. Rejected because the potentiometer is already wired to GPIO 4 per the hardware baseline and a physical knob gives immediate, tactile feedback without requiring a browser session.

### 9. Provide web-based gain override controls during browser listening
While listening to the live audio stream in a browser, users may find the volume suboptimal and lack physical access to the board's potentiometer. To support remote adjustment without sacrificing the physical control baseline:
- The browser player page includes a checkbox to activate the override and a slider to select a gain percentage (0..=100%).
- Changes are submitted via `POST /gain` (or `GET /gain`) supporting both URI query parameters (`?override=true&value=50`) and JSON request bodies (`{"override": true, "value": 50}`). The endpoint also handles `OPTIONS` for CORS preflight.
- The firmware stores the override state atomically in `SharedAudioBuffer` (`gain_override_active`, `gain_override_bits`, `gain_override_percent`).
- Core 1 continues polling the physical potentiometer every frame so that its reading is always fresh, but `SharedAudioBuffer::current_gain()` applies the web override whenever active.
- Both the OLED display in STATUS_MODE and the `/status` API endpoint reflect the active gain.
- The override persists in memory until the user unchecks the checkbox on the web page or the board reboots, at which point the device immediately falls back to the physical potentiometer.

**Alternative considered:** persisting the web override to NVS flash across reboots. Rejected because the physical potentiometer is the hardware source of truth on boot; storing override across power cycles could result in confusing silent or high-gain states upon restart.
 
### 10. Onboard RGB LED (GPIO 38) boot status indicator
The Waveshare ESP32-S3-WROOM-1-N8R8 development board includes an addressable WS2812 RGB LED tied to GPIO 38. Upon initial power-on or hardware reset, the default boot state or hardware pull can leave the LED glowing red. To provide clear visual feedback during startup and keep the board unobtrusive during operation:
- The firmware manages GPIO 38 via the ESP32-S3 RMT (Remote Control) peripheral (`TxChannelDriver` with `BytesEncoder` configured for WS2812 pulse timings).
- At the start of `App::run`, the LED is initialized and immediately set to blue (`[0, 0, 64]`) to indicate ongoing hardware, Wi-Fi, audio buffer, and peripheral initialization.
- As soon as all subsystems are initialized and before entering the main loop, the LED is turned completely off (`[0, 0, 0]`).

**Alternative considered:** using external crates like `ws2812-esp32-rmt-driver`. Rejected due to crate dependency conflicts with the patched `esp-idf-hal 0.47`; a lightweight RMT driver in `src/led.rs` using standard ESP-IDF RMT primitives keeps dependencies minimal and rock-solid.

### 11. NTP System Time Synchronization Service
To produce accurate, human-readable, and sortable filenames (`YYYYMMDD_HHMMSS.wav`) for recorded audio, the board's internal real-time clock (RTC) must be synchronized to UTC:
- **Client Configuration:** The firmware utilizes the ESP-IDF SNTP service (`esp-idf-svc::sntp::EspSntp`) configured for public NTP pools (`pool.ntp.org`).
- **Connection Trigger:** Once Wi-Fi is successfully connected, the NTP client is initialized and starts time synchronization asynchronously on Core 0.
- **Timestamp Formatting:** When arming or starting a recording, the system reads current wall-clock UTC time (`time()`, `gmtime_r`) to format `YYYYMMDD_HHMMSS.wav`.
- **Fault Tolerance & Fallback:** If Wi-Fi is disconnected or NTP synchronization times out, the system logs a warning and falls back to uptime-based or monotonic timestamps (e.g. `19700101_HHMMSS.wav`) rather than blocking device operation or crashing.
- **Periodic Re-sync:** To prevent RTC drift across long sessions, the SNTP service continues operating periodically in the background (or re-syncs once daily at midnight UTC).
- **Silent Background Operation:** Failures during background re-sync are logged to console and do not interrupt audio streaming, recording, or OLED display.

**Alternative considered:** using an external battery-backed I2C RTC (e.g., DS3231). Rejected because it requires extra hardware components, wiring, and I2C address management, whereas the device already possesses Wi-Fi connectivity and can synchronize seamlessly over NTP.

**Alternative considered:** blocking boot or recording until NTP succeeds. Rejected because the device must remain operational even in offline environments or during temporary network outages.

### 12. Hardware & Web Recording Controls in SD_MODE
Recording audio to the SD card requires clear state management and intuitive controls across both the physical board and the web interface:
- **Prerequisite State:** The device must be in `SD_MODE` and the SD card must be mounted and writable.
- **Physical Button 2 (GPIO 7) — Start/Stop:**
  - In `SD_MODE` while idle: pressing Button 2 starts recording to `/audio/YYYYMMDD_HHMMSS.wav`. The OLED displays an active recording status (e.g., recording indicator and elapsed time).
  - While recording is active: pressing Button 2 again stops recording. The recording worker flushes all pending frames, updates the 44-byte WAV header with the final chunk sizes, flushes the FATFS buffers, and finalizes the file.
- **Physical Button 3 (GPIO 6) — Cancel & Discard:**
  - While recording is active: pressing Button 3 immediately cancels the recording. Audio capture and writing for the file are aborted, the file is closed, and the incomplete file is deleted from `/audio`, preventing corrupt or partial recordings from cluttering the SD card.
- **Web UI Recording Controls:**
  - The device web interface exposes controls to start and stop recordings remotely (`POST /api/recording/start` and `POST /api/recording/stop`), sharing the same recording state machine as physical button interactions.

**Alternative considered:** keeping cancelled recordings marked as `.partial`. Rejected because incomplete files waste limited SD storage and require manual cleanup by the user.

### 13. Stored Recording Browsing, Browser Playback, and Deletion
Users need to inspect, play back, and manage recorded files both on the physical device and via the web browser:
- **On-Device Browsing (SD_MODE):**
  - When not recording, the `SD_MODE` OLED screen displays the list of files found in `/audio`, ordered in reverse chronological order (newest recordings first).
  - Button 2 (GPIO 7) navigates among existing recordings.
  - Button 3 (GPIO 6) skips to the next recording in the list.
- **On-Device Playback:**
  - Pressing Button 2 on the currently selected recording initiates audio playback, and pressing Button 2 again stops playback.
- **On-Device Safe Deletion:**
  - To prevent accidental deletion on the board, deleting the currently selected recording requires a simultaneous long press of Button 2 and Button 3 (chorded long press). Upon detection, the file is unlinked from the SD card and the OLED file list updates immediately.
- **Web Interface File Management & Direct Playback:**
  - `GET /api/recordings`: Returns a JSON list of existing files in `/audio` including filename, size, and date/time.
  - `GET /recordings/{filename}`: Serves the `.wav` file with `Content-Type: audio/wav`, allowing native audio playback directly inside standard browser `<audio>` elements without third-party plugins.
  - `DELETE /api/recordings/{filename}`: Deletes the specified recording from the SD card.

**Alternative considered:** single-button long press for on-device deletion. Rejected because holding a single navigation button can easily be triggered accidentally; requiring both Button 2 and Button 3 to be held simultaneously provides an intentional, safe confirmation chord.

## Risks / Trade-offs

- [Audio buffer pressure under SD write stalls] → Use PSRAM-backed buffers and a dual-core split so the real-time capture path remains stable while recording or network activity happens.
- [Browser compatibility across stream formats] → Prefer a simple browser-compatible endpoint and keep the stream payload standardized, with minimal fallback logic where needed.
- [Limited memory and CPU availability] → Keep the browser-serving layer minimal and focus the firmware on the critical live path first.
- [Potential file-system and recording issues] → Validate SD card write reliability during recording and finalize files cleanly when stopped. Cancelled recordings are immediately deleted to avoid orphaned fragments.
- [GPIO wiring errors] → Follow the fixed mapping exactly and avoid the reserved PSRAM pins to prevent device instability or crashes.
- [NTP sync latency or network failure] → Initialize NTP asynchronously on Core 0; if Wi-Fi or NTP is unavailable, log a warning and fall back to monotonic uptime timestamps without blocking device operation.
- [Accidental file deletion on hardware] → Require a simultaneous two-button long press (Button 2 + Button 3) so single button presses cannot delete recordings.
- [Concurrent SD card access] → Synchronize FATFS operations with a mutex so recording writes, web file downloads, and deletions do not corrupt the filesystem.

## Migration Plan

No migration is required for this initial project version. The change introduces the device’s browser-accessible audio pipeline and storage model from a greenfield state, while preserving the original hardware and architecture contract.

## Open Questions

- The exact OLED layout for STATUS_MODE sensor readings may require iteration once real I2S and PIR data is available on the hardware, given the 128x32 pixel constraint.

