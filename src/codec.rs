#![allow(dead_code)]
#![allow(private_interfaces)]
use std::{
    io::{self, Write},
    fmt::{self, Debug, Formatter},
    rc::Rc,
    cell::RefCell,
};

use crate::*;

use headers::{VorbisIdentificationHeader, VorbisMode, VorbisSetupHeader};
use bitrate::{VorbisBitrateManagerInfo, VorbisBitrateManagerState};
use codebook::{StaticCodeBook, CodeBook};
use floor::{VorbisFloor, VorbisLookFloor};
use mapping::VorbisMapping;
use residue::{VorbisResidue, VorbisLookResidue};
use psy::{VorbisInfoPsyGlobal, VorbisLookPsyGlobal, VorbisInfoPsy, VorbisLookPsy};
use envelope::VorbisEnvelopeLookup;
use mdct::MdctLookup;
use drft::DrftLookup;
use highlevel::HighlevelEncodeSetup;

/// * VorbisCodecSetup
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisCodecSetup {
    /// Vorbis supports only short and long blocks, but allows the encoder to choose the sizes
    pub block_size: [i32; 2],

    /// Static codebooks
    pub static_codebooks: Vec<StaticCodeBook>,

    /// Floors
    pub floors: Vec<Rc<VorbisFloor>>,

    /// Residues
    pub residues: Vec<Rc<VorbisResidue>>,

    /// Maps
    pub maps: Vec<Rc<VorbisMapping>>,

    /// Modes
    pub modes: Vec<VorbisMode>,

    /// Codebooks
    pub fullbooks: Rc<RefCell<Vec<Rc<CodeBook>>>>,

    /// Encode only
    pub psys: [Rc<VorbisInfoPsy>; 4],
    pub psy_g: Rc<VorbisInfoPsyGlobal>,

    pub bitrate_manager_info: VorbisBitrateManagerInfo,

    /// used only by vorbisenc.c. It's a highly redundant structure, but improves clarity of program flow.
    pub highlevel_encode_setup: HighlevelEncodeSetup,

    /// painless downsample for decode
    pub halfrate_flag: bool,
}

fn to_vec_rc<T>(src: &[T]) -> Vec<Rc<T>>
where
    T: Clone + Sized {
    src.iter().map(|v|Rc::new(v.clone())).collect()
}

impl VorbisCodecSetup {
    pub fn new(setup_header: &VorbisSetupHeader) -> io::Result<Self> {
        Ok(Self {
            static_codebooks: setup_header.static_codebooks.clone(),
            floors: to_vec_rc(&setup_header.floors),
            residues: to_vec_rc(&setup_header.residues),
            maps: to_vec_rc(&setup_header.maps),
            modes: setup_header.modes.clone(),
            ..Default::default()
        })
    }

    pub fn set_encoder_mode(&mut self) -> io::Result<()> {
        let mut fullbooks = self.fullbooks.borrow_mut();
        fullbooks.resize(self.static_codebooks.len(), Rc::default());
        for (i, static_codebook) in self.static_codebooks.iter().enumerate() {
            fullbooks[i] = Rc::new(CodeBook::new(true, static_codebook)?);
        }
        Ok(())
    }

    pub fn set_decoder_mode(&mut self) -> io::Result<()> {
        let mut fullbooks = self.fullbooks.borrow_mut();
        fullbooks.resize(self.static_codebooks.len(), Rc::default());
        for (i, static_codebook) in self.static_codebooks.iter().enumerate() {
            fullbooks[i] = Rc::new(CodeBook::new(false, static_codebook)?);
        }
        Ok(())
    }

    pub fn psyset_setup(
        &mut self,
        n: usize,
        base_setting: f64,
        psy_noise_normal_start: &[i32],
        psy_noise_normal_partition: &[i32],
        psy_noise_normal_thresh: &[f64],
    ) {
        let hi = &self.highlevel_encode_setup;
        let psy = self.psys[n].clone();
        let mut psy = *psy;
        psy.block_flag = n as i32 >> 1;

        if hi.noise_normalize_p != 0 {
            let is = base_setting as usize;
            psy.normal_p = 1;
            psy.normal_start = psy_noise_normal_start[is];
            psy.normal_partition = psy_noise_normal_partition[is];
            psy.normal_thresh = psy_noise_normal_thresh[is];
        }
        self.psys[n] = Rc::new(psy);
    }
}

/// * The `VorbisInfo` structure
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisInfo {
    pub version: i32,
    pub channels: i32,
    pub sample_rate: i32,

    /* The below bitrate declarations are *hints*.
       Combinations of the three values carry the following implications:

       all three set to the same value:
         implies a fixed rate bitstream
       only nominal set:
         implies a VBR stream that averages the nominal bitrate.  No hard
         upper/lower limit
       upper and or lower set:
         implies a VBR bitstream that obeys the bitrate limits. nominal
         may also be set to give a nominal rate.
       none set:
         the coder does not care to speculate.
    */

    pub bitrate_upper: i32,
    pub bitrate_nominal: i32,
    pub bitrate_lower: i32,
    pub bitrate_window: i32,

    pub codec_setup: VorbisCodecSetup,
}

impl VorbisInfo {
    pub fn new(identification_header: &VorbisIdentificationHeader, setup_header: &VorbisSetupHeader) -> io::Result<Self> {
        let id = identification_header;
        Ok(Self {
            version: id.version,
            channels: id.channels,
            sample_rate: id.sample_rate,
            bitrate_upper: id.bitrate_upper,
            bitrate_nominal: id.bitrate_nominal,
            bitrate_lower: id.bitrate_lower,
            bitrate_window: 0,
            codec_setup: VorbisCodecSetup::new(setup_header)?,
        })
    }

    pub fn psy_global_look(&self) -> VorbisLookPsyGlobal {
        let codec_setup = &self.codec_setup;
        VorbisLookPsyGlobal::new(-9999.0, self.channels, codec_setup.psy_g.clone())
    }
}

/// * The private part of the `VorbisDspState` for `libvorbis-1.3.7`
#[derive(Debug)]
struct VorbisDspStatePrivate<W>
where
    W: Write + Debug
{
    pub envelope: Option<VorbisEnvelopeLookup>,
    pub window: [i32; 2],
    pub transform: [[MdctLookup; 2]; 1],
    pub fft_look: Vec<DrftLookup>,
    pub modebits: i32,

    pub flr_look: Vec<VorbisLookFloor>,
    pub residue_look: Vec<VorbisLookResidue>,
    pub psy_look: Vec<VorbisLookPsy>,
    pub psy_g_look: VorbisLookPsyGlobal,

    pub bitrate_manager_state: VorbisBitrateManagerState<W>,
}

impl<W> VorbisDspStatePrivate<W>
where
    W: Write + Debug
{
    /// Analysis side code, but directly related to blocking. Thus it's
    /// here and not in analysis.c (which is for analysis transforms only).
    /// The init is here because some of it is shared
    pub fn new(vorbis_dsp_state: &VorbisDspState<W>) -> io::Result<Self> {
        let vorbis_info = &vorbis_dsp_state.vorbis_info;
        let codec_setup = &vorbis_info.codec_setup;
        let for_encode = vorbis_dsp_state.for_encode;
        let block_size = [codec_setup.block_size[0] as usize, codec_setup.block_size[1] as usize];
        let hs = if codec_setup.halfrate_flag {1} else {0};

        assert!(codec_setup.modes.len() > 0);
        assert!(block_size[0] >= 64);
        assert!(block_size[1] >= block_size[0]);

        let modebits = ilog!(codec_setup.modes.len() - 1);
        let transform = [
            [
                MdctLookup::new(block_size[0] >> hs),
                MdctLookup::new(block_size[1] >> hs),
            ],
        ];
        let window = [
            ilog!(block_size[0]) - 7,
            ilog!(block_size[1]) - 7
        ];
        let fft_look;
        if for_encode {
            fft_look = [
                DrftLookup::new(block_size[0]),
                DrftLookup::new(block_size[1]),
            ].to_vec();
        } else {
            fft_look = Vec::new();
        }
        let vorbis_info = &vorbis_dsp_state.vorbis_info;
        let codec_setup = &vorbis_info.codec_setup;

        let mut flr_look = Vec::<VorbisLookFloor>::with_capacity(codec_setup.floors.len());
        let mut residue_look = Vec::<VorbisLookResidue>::with_capacity(codec_setup.residues.len());
        let mut psy_look = Vec::<VorbisLookPsy>::with_capacity(codec_setup.psys.len());
        let psy_g_look = vorbis_info.psy_global_look();

        for floor in codec_setup.floors.iter() {
            flr_look.push(VorbisLookFloor::look(floor.clone()));
        }
        for residue in codec_setup.residues.iter() {
            residue_look.push(VorbisLookResidue::look(residue.clone(), vorbis_dsp_state));
        }
        for psy in codec_setup.psys.iter() {
            psy_look.push(VorbisLookPsy::new(psy.clone(), &*codec_setup.psy_g, block_size[psy.block_flag as usize] / 2, vorbis_info.sample_rate as u32));
        }

        Ok(Self {
            modebits,
            window,
            transform,
            fft_look,
            flr_look,
            residue_look,
            psy_look,
            psy_g_look,
            ..Default::default()
        })
    }
}

impl<W> Default for VorbisDspStatePrivate<W>
where
    W: Write + Debug
{
    fn default() -> Self {
        use std::{mem, ptr::{write, addr_of_mut}};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).envelope), None);
            write(addr_of_mut!((*ptr).transform), [[MdctLookup::default(), MdctLookup::default()]]);
            write(addr_of_mut!((*ptr).psy_g_look), VorbisLookPsyGlobal::default());
            write(addr_of_mut!((*ptr).bitrate_manager_state), VorbisBitrateManagerState::default());
            ret_z.assume_init()
        }
    }
}

/// * Am I going to reinvent the `libvorbis` wheel myself?
pub struct VorbisDspState<W>
where
    W: Write + Debug
{
    pub for_encode: bool,
    pub vorbis_info: VorbisInfo,

    pub pcm: Vec<Vec<f32>>,
    pub pcm_ret: Vec<Vec<f32>>,
    pub pcm_storage: usize,
    pub pcm_current: usize,
    pub pcm_returned: usize,

    pub preextrapolate: i32,
    pub eofflag: bool,

    /// previous window size
    pub l_w: usize,

    /// current window size
    pub w: usize,
    pub n_w: usize,
    pub center_w: usize,

    pub granulepos: i64,

    pub glue_bits: i64,
    pub time_bits: i64,
    pub floor_bits: i64,
    pub res_bits: i64,

    pub backend_state: VorbisDspStatePrivate<W>,
}

impl<W> VorbisDspState<W>
where
    W: Write + Debug
{
    pub fn new(vorbis_info: VorbisInfo, for_encode: bool) -> io::Result<Box<Self>> {
        let codec_setup = &vorbis_info.codec_setup;
        let pcm_storage = codec_setup.block_size[1] as usize;
        let pcm = vecvec![[0.0; pcm_storage]; vorbis_info.channels as usize];
        let pcm_ret = vecvec![[0.0; pcm_storage]; vorbis_info.channels as usize];
        let center_w = (codec_setup.block_size[1] / 2) as usize;
        let pcm_current = center_w;

        let mut ret = Box::new(Self {
            for_encode,
            vorbis_info,
            pcm,
            pcm_ret,
            pcm_storage,
            pcm_current,
            center_w,
            ..Default::default()
        });
        ret.backend_state = VorbisDspStatePrivate::new(&ret)?;
        if for_encode {
            ret.vorbis_info.codec_setup.set_encoder_mode()?;
        } else {
            ret.vorbis_info.codec_setup.set_decoder_mode()?;
        }
        Ok(ret)
    }
}

impl<W> Debug for VorbisDspState<W>
where
    W: Write + Debug
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisDspState")
        .field("for_encode", &self.for_encode)
        .field("vorbis_info", &self.vorbis_info)
        .field("pcm", &NestVecFormatter::new_level1(&self.pcm))
        .field("pcm_ret", &NestVecFormatter::new_level1(&self.pcm_ret))
        .field("pcm_storage", &self.pcm_storage)
        .field("pcm_current", &self.pcm_current)
        .field("pcm_returned", &self.pcm_returned)
        .field("preextrapolate", &self.preextrapolate)
        .field("eofflag", &self.eofflag)
        .field("l_w", &self.l_w)
        .field("w", &self.w)
        .field("n_w", &self.n_w)
        .field("center_w", &self.center_w)
        .field("granulepos", &self.granulepos)
        .field("glue_bits", &self.glue_bits)
        .field("time_bits", &self.time_bits)
        .field("floor_bits", &self.floor_bits)
        .field("res_bits", &self.res_bits)
        .field("backend_state", &self.backend_state)
        .finish()
    }
}

impl<W> Default for VorbisDspState<W>
where
    W: Write + Debug
{
    fn default() -> Self {
        use std::{mem, ptr::{write, addr_of_mut}};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).vorbis_info), VorbisInfo::default());
            write(addr_of_mut!((*ptr).pcm), Vec::new());
            write(addr_of_mut!((*ptr).pcm_ret), Vec::new());
            write(addr_of_mut!((*ptr).backend_state), VorbisDspStatePrivate::default());
            ret_z.assume_init()
        }
    }
}
