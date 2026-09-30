## Purpose

This capability defines the browser-accessible audio pipeline for the ESP32-S3 device: live streaming first, then local Ogg Opus recording to SD, and finally playback of stored audio back to the browser without requiring external resources.

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

### Requirement: Device records live audio to SD card in Ogg Opus format
The system SHALL record captured audio to the MicroSD card as Ogg Opus files while the device is operating in recording mode.

#### Scenario: Recording begins
- **WHEN** the user starts recording
- **THEN** the system SHALL create an Ogg Opus output file on the MicroSD card and write audio frames continuously until recording stops

#### Scenario: Recording stops
- **WHEN** the user stops recording or the device is commanded to stop
- **THEN** the system SHALL finalize the file and make it available for later playback

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

