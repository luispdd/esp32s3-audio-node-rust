pub mod frame;
pub mod mic;
pub mod playback;
pub mod recorder;
pub mod stream;

pub use frame::AudioFrame;
pub use mic::{
    convert_i2s_bytes_to_pcm16, convert_i2s_bytes_to_pcm16_with_gain, get_noise_threshold,
    set_noise_threshold, Microphone, NOISE_DETECTION_THRESHOLD_PERCENT,
};
pub use playback::{ActivePlaybackInfo, PlaybackController, PlaybackState};
pub use recorder::{ActiveRecordingInfo, RecordingController, RecordingState, WavWriter};
pub use stream::{create_wav_header, LiveAudioStream, SharedAudioBuffer, DEFAULT_BUFFER_CAPACITY};

