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
- Record microphone audio locally in Ogg Opus format to an SD card.
- Expose stored recordings through a browser-accessible playback path.
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
2. SD-card recording in Ogg Opus
3. Playback of stored recordings to browser

This keeps the product complete in scope while reducing risk and validating the critical audio path early.

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

### 3. Use Ogg Opus for offline recording
The system will record audio as Ogg Opus files to the SD card, matching the project’s initial specification and the expected compact-storage requirement for long-form audio logging.

**Alternative considered:** PCM or WAV recording. Rejected because the project explicitly targets Ogg Opus for efficient storage and aligns with the original architecture decision.

### 4. Respect the original audio pipeline configuration
The design will keep the capture configuration aligned with the initial spec: 16 kHz mono audio, 24-bit data packed into a 32-bit slot, converted into 16-bit PCM frames for streaming and compression, and Opus encoded in voice mode at roughly 16-24 kbps VBR.

**Alternative considered:** deviating from the original sample and encoding setup. Rejected because the project’s hardware and architecture assumptions depend on that configuration for stability and audio quality.

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

## Risks / Trade-offs

- [Audio buffer pressure under SD write stalls] → Use PSRAM-backed buffers and a dual-core split so the real-time capture path remains stable while recording or network activity happens.
- [Browser compatibility across stream formats] → Prefer a simple browser-compatible endpoint and keep the stream payload standardized, with minimal fallback logic where needed.
- [Limited memory and CPU availability] → Keep the browser-serving layer minimal and focus the firmware on the critical live path first.
- [Potential file-system and recording issues] → Validate SD card write reliability during recording and finalize files cleanly when stopped.
- [GPIO wiring errors] → Follow the fixed mapping exactly and avoid the reserved PSRAM pins to prevent device instability or crashes.

## Migration Plan

No migration is required for this initial project version. The change introduces the device’s browser-accessible audio pipeline and storage model from a greenfield state, while preserving the original hardware and architecture contract.

## Open Questions

- SD_MODE file navigation, playback controls, and deletion are intentionally deferred from the initial mode-system phase. Their interaction design will be defined when the recording and playback tasks are undertaken in phases 4 and 5.
- The exact OLED layout for STATUS_MODE sensor readings may require iteration once real I2S and PIR data is available on the hardware, given the 128x32 pixel constraint.
