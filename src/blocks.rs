#![allow(dead_code)]
#![allow(private_interfaces)]
use std::{
    fmt::Debug,
    io::Write,
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
#[derive(Debug)]
pub struct VorbisBlock<W>
where
    W: Write + Debug
{
    pub pcm: Vec<Vec<f32>>,
    pub ogg_pack_buffer: Rc<RefCell<BitWriterCursor>>,

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
        let mut ret = Self {
            ogg_pack_buffer: Rc::default(),
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

impl<W> Default for VorbisBlock<W>
where
    W: Write + Debug
{
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
