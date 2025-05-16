use crate::vorbis::VorbisInfo;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct VorbisEnvelopeFilterState {
    pub ampbuf: [f32; 17],
    pub ampptr: usize,
    pub nearDC: [f32; 15],
    pub nearDC_acc: f32,
    pub nearDC_partialacc: f32,
    pub nearptr: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct VorbisEnvelopeBand {
    pub begin: i32,
    pub end: i32,
    pub window: Vec<f32>,
    pub total: f32,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct VorbisEnvelopeLookup {
    pub ch: i32,
    pub winlength: i32,
    pub searchstep: i32,
    pub minenergy: f32,
    pub mdct: MdctLookup,
    pub mdct_win: Vec<f32>,
    pub envelope_band: Vec<VorbisEnvelopeBand>,
    pub envelope_filter_state: Vec<VorbisEnvelopeFilterState>,
    pub stretch: i32,
    pub mark: Vec<i32>,
    pub storage: i32,
    pub current: i32,
    pub curmark: i32,
    pub cursor: i32,
}

impl VorbisEnvelopeLookup {
    pub fn new(into: &VorbisInfo) -> Self {

    }
}