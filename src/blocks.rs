#![allow(dead_code)]
#![allow(private_interfaces)]
use std::{
    fmt::{self, Debug, Formatter},
    rc::Rc,
    cell::RefCell,
};

pub const BLOCKTYPE_IMPULSE    : i32 = 0;
pub const BLOCKTYPE_PADDING    : i32 = 1;
pub const BLOCKTYPE_TRANSITION : i32 = 0;
pub const BLOCKTYPE_LONG       : i32 = 1;

use crate::*;
use codec::VorbisDspState;
use bitwise::BitWriterCursor;

#[derive(Default, Debug, Clone)]
pub struct VorbisBlockInternal {
    pub pcmdelay: Vec<Vec<f32>>,
    pub ampmax: f32,
    pub blocktype: i32,
    pub packetblob: [Rc<RefCell<BitWriterCursor>>; PACKETBLOBS],
}

/// Necessary stream state for linking to the framing abstraction
#[allow(non_snake_case)]
pub struct VorbisBlock {
    pub pcm: Vec<Vec<f32>>,
    pub ogg_pack_buffer: Rc<RefCell<BitWriterCursor>>,

    pub lW: usize,
    pub W: usize,
    pub nW: usize,
    pub pcmend: usize,

    pub mode: i32,

    pub eofflag: bool,
    pub granulepos: u64,
    pub sequence: u32,
    pub ogg_stream_id: u32,

    /// For read-only access of configuration
    pub vorbis_dsp_state: Rc<VorbisDspState>,

    pub glue_bits: i32,
    pub time_bits: i32,
    pub floor_bits: i32,
    pub res_bits: i32,

    pub internal: Option<VorbisBlockInternal>,
}

impl VorbisBlock {
    pub fn new(vorbis_dsp_state: Rc<VorbisDspState>, ogg_stream_id: u32) -> Self {
        let mut ret = Self {
            ogg_pack_buffer: Rc::default(),
            ogg_stream_id,
            vorbis_dsp_state: vorbis_dsp_state.clone(),
            internal: None,
            ..Default::default()
        };
        if vorbis_dsp_state.for_encode {
            ret.internal = Some(VorbisBlockInternal {
                pcmdelay: Vec::new(),
                ampmax: -9999.0,
                blocktype: 0,
                packetblob: std::array::from_fn(|i|{
                    if i == PACKETBLOBS / 2 {
                        ret.ogg_pack_buffer.clone()
                    } else {
                        Rc::default()
                    }
                }),
            })
        }

        ret
    }
}

impl Debug for VorbisBlock {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisBlock")
        .field("pcm", &NestVecFormatter::new_level1(&self.pcm))
        .field("ogg_pack_buffer", &self.ogg_pack_buffer)
        .field("lW", &self.lW)
        .field("W", &self.W)
        .field("nW", &self.nW)
        .field("pcmend", &self.pcmend)
        .field("mode", &self.mode)
        .field("eofflag", &self.eofflag)
        .field("granulepos", &self.granulepos)
        .field("sequence", &self.sequence)
        .field("ogg_stream_id", &self.ogg_stream_id)
        .field("vorbis_dsp_state", &self.vorbis_dsp_state)
        .field("glue_bits", &self.glue_bits)
        .field("time_bits", &self.time_bits)
        .field("floor_bits", &self.floor_bits)
        .field("res_bits", &self.res_bits)
        .field("internal", &self.internal)
        .finish()
    }
}

impl Default for VorbisBlock {
    fn default() -> Self {
        use std::{mem, ptr::{write, addr_of_mut}};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).ogg_pack_buffer), Rc::default());
            write(addr_of_mut!((*ptr).internal), None);
            ret_z.assume_init()
        }
    }
}
