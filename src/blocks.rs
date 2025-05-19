#![allow(dead_code)]
use std::{
    fmt::Debug,
    io::Write,
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
pub struct VorbisBlock<'a, 'b, 'c, W>
where
    W: Write + Debug
{
    pub pcm: Vec<Vec<f32>>,
    pub ogg_stream_writer: OggStreamWriter<W>,

    pub lw: i32,
    pub w: i32,
    pub nw: i32,
    pub pcmend: i32,
    pub mode: i32,

    pub eofflag: bool,
    pub granulepos: i64,
    pub sequence: i64,

    /// For read-only access of configuration
    pub vorbis_dsp_state: &'a VorbisDspState<'a, 'b, 'c, W>,

    pub glue_bits: i32,
    pub time_bits: i32,
    pub floor_bits: i32,
    pub res_bits: i32,

    internal: Option<VorbisBlockInternal>,
}

impl<'a, 'b, 'c, W> VorbisBlock<'a, 'b, 'c, W>
where
    W: Write + Debug
{
    pub fn new(vorbis_dsp_state: &'a VorbisDspState<'a, 'b, 'c, W>, writer: W, ogg_stream_id: u32) -> Self {
        Self {
            pcm: Vec::new(),
            ogg_stream_writer: OggStreamWriter::new(writer, ogg_stream_id),
            lw: 0,
            w: 0,
            nw: 0,
            pcmend: 0,
            mode: 0,
            eofflag: false,
            granulepos: 0,
            sequence: 0,
            vorbis_dsp_state,
            glue_bits: 0,
            time_bits: 0,
            floor_bits: 0,
            res_bits: 0,
            internal: if vorbis_dsp_state.for_encode {
                Some(VorbisBlockInternal {
                    pcmdelay: Vec::new(),
                    ampmax: -9999.0,
                    blocktype: 0,
                })
            } else {
                None
            }
        }
    }
}

impl<'a, 'b, 'c, W> Default for VorbisBlock<'a, 'b, 'c, W>
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
