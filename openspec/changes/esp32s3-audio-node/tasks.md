## 1. Foundation and Configuration

- [x] 1.1 Define the local Wi-Fi credentials file format and verify it is excluded from source control
- [x] 1.2 Configure the ESP32-S3 firmware project for the required audio, storage, and network settings and verify the build compiles
- [x] 1.3 Verify the hardware baseline from the project specification is implemented exactly: INMP441 on GPIO 14/15/16, MicroSD on GPIO 12/11/13/10, OLED on GPIO 8/9, PIR on GPIO 3, ADC on GPIO 4, buttons on GPIO 5/6/7, and GPIO 33-37 remain unused
- [x] 1.4 Verify the dual-core architecture is honored: Core 1 for real-time capture and display, Core 0 for networking, encoding, and storage, with PSRAM-backed buffers used for audio and codec work
- [x] 1.5 Confirm the device reports readiness on the OLED or logs and verifies the required hardware is initialized before streaming or recording starts

## 2. Basic System Setup and Mode Switching

- [x] 2.1 Implement the wifi connection process and verify it by displaying in the screen the current IP address obtained by the DHCP local network's router.
- [x] 2.2 Implement the Button 1 (Mode): **GPIO 5** functionality. It will initially just switch from STATUS_MODE to LIVE_MODE and SD_MODE.
- [x] 2.3 Implement the initial display of information in the OLED screen for the LIVE_MODE, displaying the title 'LIVE_MODE' and the current IP address of the device.
- [ ] 2.4 Refactor the project source structure to follow Rust best practices for modularity and maintainability. The current `src/app.rs` (460+ lines) is a clear code smell — it conflates hardware types, domain logic, display rendering, SD probing, audio types, and application orchestration in a single file. The target module layout is:
  - `src/modes.rs` — `DeviceMode` enum and its cycling/display logic
  - `src/status.rs` — `SystemStatus` struct and its sensor aggregation helpers
  - `src/display.rs` — OLED probe, initialization, and per-mode rendering (see task 2.5)
  - `src/sd.rs` — SD card lifecycle and probe logic (see task 2.8)
  - `src/audio/mod.rs` — audio module root
  - `src/audio/frame.rs` — `AudioFrame` data type
  - `src/audio/stream.rs` — `LiveAudioStream` and stub capture logic
  - `src/app.rs` — thin orchestration only: takes peripherals, wires modules together, runs the main loop
  - `src/main.rs` — entry point, unchanged
  All existing unit tests must be relocated to their respective modules and continue to pass. No behavioral changes should be introduced by this task.
- [ ] 2.5 Extract screen management into a dedicated `src/display.rs` module with a clear, extensible API. The module must be designed to support future additions of new modes and richer per-mode content without requiring changes to the core application loop.
- [ ] 2.6 Implement the SD_MODE screen. It will verify that the SD card exists and can be mounted, that the `/audio` folder exists, and then display the list of recorded files inside it. This is an initial view; future tasks will add file navigation, playback controls, and deletion.
- [ ] 2.7 Implement the STATUS_MODE display. It will show the live state of each sensor: Wi-Fi connectivity, microphone signal presence (real I2S data, not a hardcoded value), PIR sensor activity (real GPIO poll, not a hardcoded false), and SD card availability. An initial real-hardware read of the PIR sensor and the microphone must be implemented to verify the functionality.
- [ ] 2.8 Extract SD card management into a dedicated `src/sd.rs` module. Move `check_sd_card_is_writable` and related SD logic into this module and fix the existing bug where `Peripherals::take()` is called redundantly inside the probe function after the main app has already taken peripherals.
- [ ] 2.9 Implement the Button 2 (Other): **GPIO 6** functionality. Will use it later, just log in the console when the button is pressed.
- [ ] 2.10 Implement the Button 3 (Other): **GPIO 7** functionality. Will use it later, just log in the console when the button is pressed.

## 3. Live Audio Streaming

- [ ] 3.1 Implement the microphone capture path and buffer management for live audio samples and verify audio frames are produced continuously
- [ ] 3.2 Implement the browser-accessible live stream endpoint or minimal serving page and verify a browser can connect to the device over Wi-Fi
- [ ] 3.3 Deliver the live audio payload to the client and verify playback is audible in the browser without external resources

## 4. Recording to SD Card

- [ ] 4.1 Implement Ogg Opus recording pipeline and verify data is written to an SD card file in the expected format
- [ ] 4.2 Add start/stop recording controls and verify a recording is created, finalized, and stored on the device
- [ ] 4.3 Validate recording behavior under the project's expected audio load and verify the file remains readable after finalization

## 5. Playback of Stored Audio

- [ ] 5.1 Expose a browser-accessible listing or direct endpoint for recorded files and verify the device serves stored content correctly
- [ ] 5.2 Implement playback of stored recordings in the browser and verify the browser can play back recorded audio without external services
- [ ] 5.3 Validate the full flow from capture to recording to storage to playback and verify the end-to-end behavior matches the spec

## 6. Hardening and Polish

- [ ] 6.1 Tune buffer sizes, latency, and reliability for real-world device operation and verify stable behavior under sustained use
- [ ] 6.2 Improve status reporting and user feedback on the OLED or browser UI and verify the system remains understandable during operation
- [ ] 6.3 Review the device behavior against the full-feature scope and identify future improvements for refinement in later iterations
