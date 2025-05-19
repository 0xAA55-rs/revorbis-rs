#![allow(dead_code)]
#![allow(private_interfaces)]
use std::{
    io::{self, Write},
    fmt::Debug,
};

use crate::*;

use headers::{VorbisIdentificationHeader, VorbisMode, VorbisSetupHeader};
use bitrate::{VorbisBitrateManagerInfo, VorbisBitrateManagerState};
use codebook::{StaticCodeBook, CodeBook};
use floor::VorbisFloor;
use mapping::VorbisMapping;
use residue::VorbisResidue;
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
    pub floors: Vec<VorbisFloor>,

    /// Residues
    pub residues: Vec<VorbisResidue>,

    /// Maps
    pub maps: Vec<VorbisMapping>,

    /// Modes
    pub modes: Vec<VorbisMode>,

    /// Codebooks
    pub fullbooks: Vec<CodeBook>,

    /// Encode only
    pub psys: [VorbisInfoPsy; 4],
    pub psy_g: VorbisInfoPsyGlobal,

    pub bitrate_manager_info: VorbisBitrateManagerInfo,

    /// used only by vorbisenc.c. It's a highly redundant structure, but improves clarity of program flow.
    pub highlevel_encode_setup: HighlevelEncodeSetup,

    /// painless downsample for decode
    pub halfrate_flag: bool,
}

impl VorbisCodecSetup {
    pub fn new(setup_header: &VorbisSetupHeader) -> io::Result<Self> {
        Ok(Self {
            static_codebooks: setup_header.static_codebooks.clone(),
            floors: setup_header.floors.clone(),
            residues: setup_header.residues.clone(),
            maps: setup_header.maps.clone(),
            modes: setup_header.modes.clone(),
            ..Default::default()
        })
    }

    pub fn set_encoder_mode(&mut self) -> io::Result<()> {
        self.fullbooks.resize(self.static_codebooks.len(), CodeBook::default());
        for (i, static_codebook) in self.static_codebooks.iter().enumerate() {
            self.fullbooks[i] = CodeBook::new(true, static_codebook)?;
        }
        Ok(())
    }

    pub fn set_decoder_mode(&mut self) -> io::Result<()> {
        self.fullbooks.resize(self.static_codebooks.len(), CodeBook::default());
        for (i, static_codebook) in self.static_codebooks.iter().enumerate() {
            self.fullbooks[i] = CodeBook::new(false, static_codebook)?;
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
        let psy = &mut self.psys[n];
        psy.block_flag = n as i32 >> 1;

        if hi.noise_normalize_p != 0 {
            let is = base_setting as usize;
            psy.normal_p = 1;
            psy.normal_start = psy_noise_normal_start[is];
            psy.normal_partition = psy_noise_normal_partition[is];
            psy.normal_thresh = psy_noise_normal_thresh[is];
        }
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
    pub fn new(identification_header: &VorbisIdentificationHeader, codec_setup: &VorbisCodecSetup) -> Self {
        Self {
            version: identification_header.version,
            channels: identification_header.channels,
            sample_rate: identification_header.sample_rate,
            bitrate_upper: identification_header.bitrate_upper,
            bitrate_nominal: identification_header.bitrate_nominal,
            bitrate_lower: identification_header.bitrate_lower,
            bitrate_window: 0,
            block_size: identification_header.block_size,
            codec_setup: codec_setup.clone()
        }
    }

    pub fn psy_global_look(&self) -> VorbisLookPsyGlobal {
        let codec_setup = &self.codec_setup;
        let info_psy_global = &codec_setup.psy_g;
        VorbisLookPsyGlobal::new(-9999.0, self.channels, info_psy_global)
    }
}

/// * The private part of the `VorbisDspState` for `libvorbis-1.3.7`
#[derive(Debug, Clone)]
struct VorbisDspStatePrivate<'a, 'b, 'c, W>
where
    W: Write + Debug
{
    pub envelope: Option<VorbisEnvelopeLookup>,
    pub window: [i32; 2],
    pub transform: [MdctLookup; 2],
    pub fft_look: Vec<DrftLookup>,
    pub modebits: i32,

    pub psy: VorbisLookPsy<'b>,
    pub psy_g_look: VorbisLookPsyGlobal<'c>,

    pub bitrate_manager_state: VorbisBitrateManagerState<'a, 'b, 'c, W>,
}

impl<W> VorbisDspStatePrivate<'_, '_, '_, W>
where
    W: Write + Debug
{
    /// Analysis side code, but directly related to blocking. Thus it's
    /// here and not in analysis.c (which is for analysis transforms only).
    /// The init is here because some of it is shared
    pub fn new(vorbis_dsp_state: &'a VorbisDspState<'_, W>) -> io::Result<Self> {
        let codec_info = &info.codec_setup;
        let hs = if codec_info.halfrate_flag {1} else {0};
        let block_size = [codec_setup.block_size[0] as usize, codec_setup.block_size[1] as usize];

        assert!(codec_info.modes.len() > 0);
        assert!(block_size[0] >= 64);
        assert!(block_size[1] >= block_size[0]);

        Ok(Self {
            envelope: None,
            modebits: ilog!(codec_info.modes.len() - 1),
            window: [
                ilog!(block_size[0]) - 7,
                ilog!(block_size[1]) - 7
            ],
            /* MDCT is tranform 0 */
            transform: [
                MdctLookup::new(block_size[0] >> hs),
                MdctLookup::new(block_size[1] >> hs)
            ],
            fft_look: if for_encode {
                [
                    DrftLookup::new(block_size[0]),
                    DrftLookup::new(block_size[1]),
                ].to_vec()
            } else {
                Vec::new()
            },
            ..Default::default()
        })
    }
}

impl<'a, 'b, 'c, W> Default for VorbisDspStatePrivate<'a, 'b, 'c, W>
where
    W: Write + Debug
{
    fn default() -> Self {
        use std::{mem, ptr::{write, addr_of_mut}};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).envelope), None);
            write(addr_of_mut!((*ptr).transform), [MdctLookup::default(), MdctLookup::default()]);
            write(addr_of_mut!((*ptr).psy), VorbisLookPsy::default());
            write(addr_of_mut!((*ptr).psy_g_look), VorbisLookPsyGlobal::default());
            write(addr_of_mut!((*ptr).bitrate_manager_state), VorbisBitrateManagerState::default());
            ret_z.assume_init()
        }
    }
}

/// * Am I going to reinvent the `libvorbis` wheel myself?
#[derive(Debug, Clone)]
pub struct VorbisDspState<'a, 'b, 'c, W>
where
    W: Write + Debug
{
    pub info: VorbisInfo,
    pub backend_state: VorbisDspStatePrivate<'a, 'b, 'c, W>,
    pub for_encode: bool,
}
