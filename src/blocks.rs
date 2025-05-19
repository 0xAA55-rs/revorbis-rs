#![allow(dead_code)]
#![allow(private_interfaces)]
use std::{
    fmt::Debug,
    io::Write,
    rc::Rc,
};

use crate::*;
use codec::VorbisDspState;
use ogg::OggStreamWriter;

#[derive(Default, Debug, Clone, PartialEq)]
struct VorbisBlockInternal {
    pub pcmdelay: Vec<Vec<f32>>,
    pub ampmax: f32,
    pub blocktype: i32,
}

/// Necessary stream state for linking to the framing abstraction
#[derive(Debug)]
pub struct VorbisBlock<W>
where
    W: Write + Debug
{
    pub pcm: Vec<Vec<f32>>,
    pub ogg_stream_writer: OggStreamWriter<W>,

    pub l_w: usize,
    pub w: usize,
    pub n_w: usize,
    pub pcmend: usize,

    pub mode: i32,

    pub eofflag: bool,
    pub granulepos: i64,
    pub sequence: i64,

    /// For read-only access of configuration
    pub vorbis_dsp_state: Rc<VorbisDspState<W>>,

    pub glue_bits: i32,
    pub time_bits: i32,
    pub floor_bits: i32,
    pub res_bits: i32,

    pub internal: Option<VorbisBlockInternal>,
}

impl<W> VorbisBlock<W>
where
    W: Write + Debug
{
    pub fn new(vorbis_dsp_state: Rc<VorbisDspState<W>>, writer: W, ogg_stream_id: u32) -> Self {
        Self {
            ogg_stream_writer: OggStreamWriter::new(writer, ogg_stream_id),
            vorbis_dsp_state: vorbis_dsp_state.clone(),
            internal: if vorbis_dsp_state.for_encode {
                Some(VorbisBlockInternal {
                    pcmdelay: Vec::new(),
                    ampmax: -9999.0,
                    blocktype: 0,
                })
            } else {
                None
            },
            ..Default::default()
        }
    }
}

impl<W> Default for VorbisBlock<W>
where
    W: Write + Debug
{
    fn default() -> Self {
        use std::{mem, ptr::{write, addr_of_mut}};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).internal), None);
            ret_z.assume_init()
        }
    }
}
