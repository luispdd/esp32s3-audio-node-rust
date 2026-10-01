## Purpose

This capability defines the browser-accessible audio pipeline for the ESP32-S3 device: live streaming first, then local standard WAV recording to SD, and finally playback of stored audio back to the browser without requiring external resources.

The implementation SHALL honor the hardware baseline defined in the project specification: a Waveshare ESP32-S3-WROOM-1-N8R8 using the mapped GPIO assignments for the INMP441 microphone, SD card, OLED, PIR, ADC, and buttons, with reserved GPIO 33-37 left unused.

## ADDED Requirements

### Requirement: Device respects the defined hardware pin map
The system SHALL use the declared GPIO assignments from the project specification for audio acquisition, SD storage, display output, motion input, potentiometer input, and button controls, and SHALL avoid GPIO 33-37 because they are reserved for the PSRAM bus.

#### Scenario: Hardware is initialized according to the design map
- **WHEN** the firmware starts and initializes the device peripherals
- **THEN** the system SHALL configure the INMP441 on GPIO 14/15/16, the SD card on GPIO 12/11/13/10, the OLED on GPIO 8/9, the PIR on GPIO 3, the potentiometer on GPIO 4, and the buttons on GPIO 5/6/7

#### Scenario: A reserved GPIO is selected
- **WHEN** a port assignment attempts to use GPIO 33, 34, 35, 36, or 37
- **THEN** the system SHALL reject that assignment or prevent the configuration from being applied because the hardware contract reserves those pins

### Requirement: Device provides live browser audio streaming
The system SHALL capture microphone audio on the ESP32-S3 and make it available to a browser client over the local Wi-Fi network as a live audio stream.

#### Scenario: Live stream starts successfully
- **WHEN** the device is powered on with valid Wi-Fi configuration and the streaming mode is enabled
- **THEN** the device SHALL begin capturing audio and expose a browser-accessible stream endpoint or equivalent mechanism for live playback

#### Scenario: Live stream is unavailable
- **WHEN** the Wi-Fi stack is not connected or the stream endpoint is not initialized
- **THEN** the system SHALL report an error state or remain in a non-streaming mode without crashing the device

### Requirement: Device records live audio to SD card in standard WAV format
The system SHALL record captured audio to the MicroSD card as standard WAV files while the device is operating in recording mode.

#### Scenario: Recording begins
- **WHEN** the user starts recording
- **THEN** the system SHALL create a standard WAV output file on the MicroSD card and write audio frames continuously until recording stops

#### Scenario: Recording stops
- **WHEN** the user stops recording or the device is commanded to stop
- **THEN** the system SHALL finalize the WAV header and make the file available for later playback

### Requirement: Device serves stored recordings to browser playback
The system SHALL allow a browser client to access stored audio files from the device and play them back without requiring external dependencies or a separate server application.

#### Scenario: Browser requests recorded file
- **WHEN** a browser connects to the device and requests a stored recording
- **THEN** the system SHALL provide the appropriate audio content over a supported browser-accessible protocol or minimal local page

#### Scenario: Browser cannot access the file
- **WHEN** the requested file is missing or the device has no valid audio source available
- **THEN** the system SHALL return an appropriate error state or empty result instead of hanging the client

### Requirement: Device exposes minimal browser UI or endpoint when needed
The system SHALL provide the minimal browser-facing functionality required to access live audio and stored recordings, including a minimal HTML page or equivalent browser-compatible endpoint when direct stream playback requires it.

#### Scenario: Browser loads the device page
- **WHEN** a user opens the device IP in a browser
- **THEN** the browser SHALL receive the required minimal content needed to connect to the live stream or recorded playback

#### Scenario: Browser receives no external dependency UI
- **WHEN** the device serves browser content
- **THEN** the served content SHALL be self-contained and not require non-local external assets or separate services

### Requirement: Potentiometer adjusts microphone gain in real time
The system SHALL continuously read the potentiometer on GPIO 4 (ADC1_CH3) and apply its value as a linear software gain multiplier to the captured audio samples, ranging from complete silence at the minimum position to maximum gain at the maximum position.

#### Scenario: Potentiometer is at minimum position
- **WHEN** the potentiometer wiper is at its lowest physical position (ADC reading at or near 0)
- **THEN** the system SHALL apply a gain multiplier of 0, producing silence in both the live stream and any recording

#### Scenario: Potentiometer is at maximum position
- **WHEN** the potentiometer wiper is at its highest physical position (ADC reading at or near full scale)
- **THEN** the system SHALL apply the maximum configured gain multiplier to the audio samples

#### Scenario: Potentiometer is at an intermediate position
- **WHEN** the potentiometer wiper is at any position between minimum and maximum
- **THEN** the system SHALL apply a gain multiplier proportional to the ADC reading, producing an audio level between silence and maximum gain

#### Scenario: STATUS_MODE screen reflects current gain
- **WHEN** the device is in STATUS_MODE and the OLED is on
- **THEN** the display SHALL show the current gain level derived from the potentiometer reading

### Requirement: Button 1 controls mode switching and display power
The system SHALL use Button 1 (GPIO 5) to cycle between device modes on a short press, and to toggle the OLED display power state on a long press.

#### Scenario: Short press cycles device modes while screen is active
- **WHEN** Button 1 is pressed and released before reaching the long-press threshold while the screen is on
- **THEN** the system SHALL advance to the next device mode in sequence

#### Scenario: Long press turns the screen off
- **WHEN** Button 1 is held down for at least the long-press threshold while the screen is on
- **THEN** the system SHALL turn off the OLED display without cycling the device mode

#### Scenario: Press turns the screen back on
- **WHEN** Button 1 is pressed while the screen is turned off
- **THEN** the system SHALL turn on the OLED display and restore the current mode view

### Requirement: Web interface allows overriding potentiometer gain
The system SHALL allow a browser user to override the physical potentiometer gain during live stream listening through the web interface, persisting until explicitly disabled or until the device reboots.

#### Scenario: User enables web gain override
- **WHEN** the user checks the gain override checkbox and selects a gain value on the browser page
- **THEN** the system SHALL apply the web-specified gain multiplier to the captured audio and reflect the override percentage in system status

#### Scenario: User disables web gain override
- **WHEN** the user unchecks the gain override checkbox
- **THEN** the system SHALL immediately revert to using the physical potentiometer's gain reading for audio capture

#### Scenario: Device reboots while override was active
- **WHEN** the device restarts
- **THEN** the system SHALL initialize with the gain override disabled, defaulting to the physical potentiometer reading

### Requirement: Onboard RGB LED indicates boot status
The system SHALL control the onboard WS2812 RGB LED (GPIO 38) to display blue during the boot sequence and turn off completely after initialization completes.

#### Scenario: Device starts up
- **WHEN** the device powers on or reboots and begins subsystem initialization
- **THEN** the system SHALL set the onboard RGB LED to blue

#### Scenario: Initialization finishes
- **WHEN** all hardware and network initialization completes and the main application loop begins
- **THEN** the system SHALL turn off the onboard RGB LED

### Requirement: Device synchronizes clock via NTP for timestamped recording
The system SHALL synchronize the internal real-time clock from an internet NTP service once connected to Wi-Fi to provide accurate UTC wall-clock timestamps for naming audio recordings in `/audio/YYYYMMDD_HHMMSS.wav`.

#### Scenario: Successful NTP synchronization
- **WHEN** the device connects to Wi-Fi and requests NTP synchronization
- **THEN** the system SHALL set the system RTC to UTC wall time and format subsequent recording filenames as `YYYYMMDD_HHMMSS.wav` based on the start timestamp

#### Scenario: NTP synchronization fails or times out
- **WHEN** Wi-Fi is disconnected or the NTP server does not respond within the timeout
- **THEN** the system SHALL log a warning and fall back to monotonic or internal timestamps without blocking device operation or crashing

#### Scenario: Periodic background re-sync
- **WHEN** the device remains connected over extended operation
- **THEN** the system SHALL periodically re-synchronize time in the background without interrupting streaming, recording, or display rendering

### Requirement: SD_MODE controls manage recording lifecycle
The system SHALL control recording via physical buttons and web controls when the device is in SD_MODE, creating WAV files under `/audio/YYYYMMDD_HHMMSS.wav` and allowing clean finalization or cancellation.

#### Scenario: Starting a recording
- **WHEN** the device is in SD_MODE and Button 2 (GPIO 7) is pressed (or web start is triggered)
- **THEN** the system SHALL start audio capture and writing to `/audio/YYYYMMDD_HHMMSS.wav`

#### Scenario: Stopping and finalizing a recording
- **WHEN** recording is in progress and Button 2 (GPIO 7) is pressed again (or web stop is triggered)
- **THEN** the system SHALL finalize the WAV header, flush SD buffers, and save the file

#### Scenario: Cancelling an in-progress recording
- **WHEN** recording is in progress and Button 3 (GPIO 6) is pressed
- **THEN** the system SHALL abort recording, close the file, and delete the partial recording from the SD card

### Requirement: SD_MODE supports on-device browsing, playback, and deletion
The system SHALL allow navigating, playing back, and deleting stored recordings directly on the device using Buttons 2 and 3 in SD_MODE.

#### Scenario: Browsing recordings in reverse chronological order
- **WHEN** the device is in SD_MODE and not recording
- **THEN** the system SHALL list recordings from `/audio` in reverse chronological order, allowing Button 2 to navigate files and Button 3 to skip to the next recording

#### Scenario: Initiating on-device playback
- **WHEN** a recording is selected in SD_MODE and Button 2 is pressed
- **THEN** the system SHALL start playing the recording, and stop playback if Button 2 is pressed again

#### Scenario: Deleting a recording with dual-button chord
- **WHEN** a recording is selected in SD_MODE and Button 2 and Button 3 are pressed and held simultaneously
- **THEN** the system SHALL delete the selected file from the SD card and update the displayed file list

### Requirement: Web interface manages stored recordings
The system SHALL expose web endpoints to list stored recordings, stream playback natively, and delete recordings remotely.

#### Scenario: Web listing and playback
- **WHEN** a browser client requests the list of recordings or streams a specific `.wav` file
- **THEN** the device SHALL return the file metadata list and stream the audio payload with `audio/wav` MIME headers for native browser playback

#### Scenario: Remote recording deletion
- **WHEN** a browser client issues a deletion request for a stored recording
- **THEN** the device SHALL remove the file from the SD card and confirm deletion

### Requirement: PIR_MODE supports motion-triggered recording
The system SHALL support a `PIR_MODE` that detects movement using the PIR sensor (GPIO 3) and automatically records audio to the SD card. When initiated via Button 2, the system provides a 10-second arming delay before activating motion monitoring.

#### Scenario: Initiating motion monitoring with 10-second arming delay
- **WHEN** the device is in PIR_MODE and Button 2 is pressed
- **THEN** the system SHALL begin a 10-second arming countdown, during which motion detection is not yet active, and activate motion monitoring once the 10 seconds elapse

#### Scenario: Movement triggers immediate recording while armed
- **WHEN** motion monitoring is active in PIR_MODE and the PIR sensor detects motion
- **THEN** the system SHALL immediately start WAV recording to `/audio/YYYYMMDD_HHMMSS.wav` on the SD card

#### Scenario: Subsequent motion resets the 20-second recording timer
- **WHEN** recording is active in PIR_MODE and new motion is detected by the PIR sensor
- **THEN** the system SHALL reset the recording duration timer to 20 seconds from the latest motion event

#### Scenario: Recording completes after 20 seconds of no motion
- **WHEN** 20 seconds have elapsed since the last detected movement without further motion
- **THEN** the system SHALL cleanly finalize the WAV recording and return to the active motion monitoring state

#### Scenario: Disarming motion monitoring or changing mode
- **WHEN** the device is in PIR_MODE (during arming countdown, active monitoring, or recording) and Button 2 is pressed again, or Button 1 is short-pressed to change mode
- **THEN** the system SHALL disarm motion monitoring (and finalize/stop any active recording)

#### Scenario: Turning screen off while monitoring remains active
- **WHEN** motion monitoring or recording is active in PIR_MODE and Button 1 is pressed for at least the long-press threshold
- **THEN** the system SHALL turn off the OLED display while keeping motion monitoring and recording active in the background

### Requirement: Web interface controls PIR_MODE lifecycle and noise threshold override
The system SHALL expose web endpoints and UI controls to remotely start and stop `PIR_MODE` and dynamically override the noise detection threshold.

#### Scenario: User starts PIR_MODE from web interface
- **WHEN** the user triggers the Start PIR command from the browser UI or sends a POST request to `/api/pir/start`
- **THEN** the device SHALL immediately transition to `PIR_MODE` (stopping audio playback if active in `SD_MODE`) and immediately start the 10-second arming countdown, exactly as if entering `PIR_MODE` and pressing Button 2

#### Scenario: User stops PIR_MODE from web interface
- **WHEN** the user triggers the Stop PIR command from the browser UI or sends a POST request to `/api/pir/stop`
- **THEN** the device SHALL stop and finalize any active recording to the SD card, disarm the PIR controller, and immediately exit `PIR_MODE` to `STATUS_MODE`

#### Scenario: User overrides noise detection threshold from web interface
- **WHEN** the user updates the noise detection threshold via the slider or presets on the browser UI, or sends a POST request to `/api/pir/threshold` with the desired percentage (1..=100)
- **THEN** the device SHALL update the active noise detection threshold in memory and apply it immediately to sound detection and PIR recording extension logic


