#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Vec<i16>,
}
