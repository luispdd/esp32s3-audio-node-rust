## Why

The ESP32-S3 audio node needs to capture microphone audio on-device and make it usable from a browser without relying on external services or a separate host machine. The project provides a compact hardware platform with audio capture, Wi-Fi connectivity, SD storage, and input controls, so the missing capability is a cohesive, browser-accessible audio pipeline that can be built in phases without losing the long-term product vision.

This proposal follows the hardware baseline defined in the project’s initial specification and preserves the original board and GPIO layout as the implementation contract. The board is a Waveshare ESP32-S3-WROOM-1-N8R8 with 8 MB flash and 8 MB Octal PSRAM, and the design relies on the mapped ports for I2S audio capture, SD card storage, OLED display, PIR, ADC, and buttons.

## What Changes

- Add a live audio streaming path from the microphone to a browser over Wi-Fi using the board’s existing audio capture and network stack.
- Add SD-card recording using standard 16 kHz 16-bit mono WAV format so audio can be stored locally on the device while preserving the original architecture split across the two ESP32-S3 cores without memory or stack contention.
- Add browser playback for recorded audio files, including a minimal device-hosted page or stream endpoint when needed.
- Keep the final product scope full-featured while delivering it in a practical sequence: live stream first, then SD-card recording, then playback from stored files.
- Store Wi-Fi credentials in a local, git-ignored config file so source code remains portable and safe.
- Preserve the hardware contract defined in the original design: INMP441 on GPIO 14/15/16, SD SPI on GPIO 12/11/13/10, OLED I2C on GPIO 8/9, PIR on GPIO 3, ADC gain control on GPIO 4, and buttons on GPIO 5/6/7 while avoiding GPIO 33-37.
- Implement a device mode system (STATUS_MODE, LIVE_MODE, SD_MODE) driven by Button 1 (GPIO 5), with a dedicated OLED display layout per mode, short-press mode rotation, and long-press display power toggling (off/on).
- Extract screen management into `src/display.rs` and SD card management into `src/sd.rs` to maintain a clean, extensible module boundary as the codebase grows.
- Add web-based gain override controls to the browser live stream player (a checkbox to enable override and a slider to set 0–100% gain) so listeners can adjust audio level remotely without touching the physical board, persisting in memory until unchecked or board restart.
- Control the onboard WS2812 RGB LED on GPIO 38: illuminate blue during hardware and network initialization, then turn off completely once the boot process finishes.
- Synchronize system time via an NTP client upon establishing Wi-Fi connection, ensuring accurate UTC timestamps for audio logs and file naming, with periodic background synchronization to minimize RTC drift.
- Record audio to MicroSD in WAV format under `/audio/YYYYMMDD_HHMMSS.wav` using the UTC start timestamp.
- Implement hardware recording controls in `SD_MODE`: Button 2 (GPIO 7) starts recording and stops/finalizes when pressed again; Button 3 (GPIO 6) cancels recording in progress and discards the file.
- Implement onboard recording browsing and playback in `SD_MODE`: Button 2 (GPIO 7) navigates existing files in reverse chronological order, Button 3 (GPIO 6) skips to the next recording, pressing Button 2 starts or stops playback of the selected file, and a simultaneous long press on Button 2 + Button 3 deletes the selected file.
- Provide web interface management for stored recordings: list recordings, play them back natively in the browser without external services, start/stop recording remotely, and delete recordings from the web UI.

## Capabilities

### New Capabilities
- `audio-streaming`: the ESP32-S3 device captures audio, exposes it to the browser with real-time gain control (physical potentiometer and browser override), syncs system time via NTP, records timestamped WAV files to SD card with physical/web controls, and provides onboard/browser navigation, playback, and file deletion without requiring external services.
- `device-mode-system`: a button-driven, OLED-displayed mode switching layer (STATUS_MODE, LIVE_MODE, SD_MODE) that provides a verifiable interactive foundation before the full audio pipeline is activated.

### Modified Capabilities
- None

## Impact

- Firmware: ESP32-S3 real-time audio capture, dual-core workload separation, Wi-Fi networking, SNTP time synchronization, WAV file writing, SD card FATFS operations (recording, directory listing, deletion), and browser-serving REST and audio endpoints.
- Hardware integration: microphone, SD card, OLED status display, PIR input, ADC adjustment channel, and multi-button control state machine (Buttons 1, 2, and 3 with short, long, and simultaneous chord combinations) using the original GPIO map.
- Browser interaction: direct playback of live streams and stored files from the device, gain override controls, recording trigger controls, and file management via a lightweight, self-contained web page.
- Configuration: a local Wi-Fi credentials file kept out of version control.
- System constraints: the implementation must respect the board’s PSRAM memory model and avoid reserved GPIO blocks to prevent bus corruption and crashes.
