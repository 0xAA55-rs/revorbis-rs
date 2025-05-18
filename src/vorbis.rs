use std::{
    io::{self, Write},
    mem,
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};

use crate::*;

use codebook::{StaticCodeBook, CodeBook};
use floor::VorbisFloor;
use mapping::VorbisMapping;
use residue::VorbisResidue;
use psy::{VorbisInfoPsy, VorbisInfoPsyGlobal, VorbisLookPsy};
use envelope::VorbisEnvelopeLookup;
use mdct::MdctLookup;
use drft::DrftLookup;
use copiablebuf::CopiableBuffer;

pub const PACKETBLOBS: usize = 15;

/// * The `VorbisIdentificationHeader` is the Vorbis identification header, the first header
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct VorbisIdentificationHeader {
    pub version: i32,
    pub channels: i32,
    pub sample_rate: i32,
    pub bitrate_upper: i32,
    pub bitrate_nominal: i32,
    pub bitrate_lower: i32,
    pub block_size: [i32; 2],
}

impl VorbisIdentificationHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader) -> Result<Self, io::Error> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x01vorbis" {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not a Vorbis identification header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let version = read_bits!(bitreader, 32);
            let channels = read_bits!(bitreader, 8);
            let sample_rate = read_bits!(bitreader, 32);
            let bitrate_upper = read_bits!(bitreader, 32);
            let bitrate_nominal = read_bits!(bitreader, 32);
            let bitrate_lower = read_bits!(bitreader, 32);
            let bs_1 = read_bits!(bitreader, 4);
            let bs_2 = read_bits!(bitreader, 4);
            let block_size = [1 << bs_1, 1 << bs_2];
            let end_of_packet = read_bits!(bitreader, 1) & 1 == 1;
            if sample_rate < 1
            || channels < 1
            || block_size[0] < 64
            || block_size[1] < block_size[0]
            || block_size[1] > 8192
            || !end_of_packet {
                Err(io::Error::new(io::ErrorKind::InvalidData, "Bad Vorbis identification header.".to_string()))
            } else {
                Ok(Self {
                    version,
                    channels,
                    sample_rate,
                    bitrate_upper,
                    bitrate_nominal,
                    bitrate_lower,
                    block_size,
                })
            }
        }
    }

    /// * Unpack from a slice
    pub fn load_from_slice(data: &[u8]) -> Result<Self, io::Error> {
        let mut bitreader = BitReader::new(data);
        Self::load(&mut bitreader)
    }
}

impl VorbisPackableObject for VorbisIdentificationHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let bs_1: u8 = ilog!(self.block_size[0] - 1);
        let bs_2: u8 = ilog!(self.block_size[1] - 1);
        let begin_bits = bitwriter.total_bits;
        write_slice!(bitwriter, b"\x01vorbis");
        write_bits!(bitwriter, self.version, 32);
        write_bits!(bitwriter, self.channels, 8);
        write_bits!(bitwriter, self.sample_rate, 32);
        write_bits!(bitwriter, self.bitrate_upper, 32);
        write_bits!(bitwriter, self.bitrate_nominal, 32);
        write_bits!(bitwriter, self.bitrate_lower, 32);
        write_bits!(bitwriter, bs_1, 4);
        write_bits!(bitwriter, bs_2, 4);
        write_bits!(bitwriter, 1, 1);
        Ok(bitwriter.total_bits - begin_bits)
    }
}

/// * The `VorbisCommentHeader` is the Vorbis comment header, the second header
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct VorbisCommentHeader {
    pub comments: Vec<String>,
    pub vendor: String,
}

impl VorbisCommentHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader) -> Result<Self, io::Error> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x03vorbis" {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let vendor_len = read_bits!(bitreader, 32);
            if vendor_len < 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad vendor string length {vendor_len}")));
            }
            let vendor = read_string!(bitreader, vendor_len as usize)?;
            let num_comments = read_bits!(bitreader, 32);
            if num_comments < 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad number of comments {num_comments}")));
            }
            let mut comments = Vec::<String>::with_capacity(num_comments as usize);
            for _ in 0..num_comments {
                let comment_len = read_bits!(bitreader, 32);
                if comment_len < 0 {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad comment string length {vendor_len}")));
                }
                comments.push(read_string!(bitreader, comment_len as usize)?);
            }
            let end_of_packet = read_bits!(bitreader, 1) & 1 == 1;
            if !end_of_packet {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("End of packet flag == {end_of_packet}")));
            }
            Ok(Self{
                comments,
                vendor,
            })
        }
    }
}

impl VorbisPackableObject for VorbisCommentHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        write_slice!(bitwriter, b"\x03vorbis");
        write_bits!(bitwriter, self.vendor.len(), 32);
        write_string!(bitwriter, self.vendor);
        write_bits!(bitwriter, self.comments.len(), 32);
        for comment in self.comments.iter() {
            write_bits!(bitwriter, comment.len(), 32);
            write_string!(bitwriter, comment);
        }
        write_bits!(bitwriter, 1, 1);
        Ok(bitwriter.total_bits - begin_bits)
    }
}

derive_index!(VorbisCommentHeader, String, comments);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VorbisMode {
    pub block_flag: bool,
    pub window_type: i32,
    pub transform_type: i32,
    pub mapping: i32,
}

impl VorbisMode {
    /// * Unpack from the bitstream
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<Self, io::Error> {
        let ret = Self {
            block_flag: read_bits!(bitreader, 1) != 0,
            window_type: read_bits!(bitreader, 16),
            transform_type: read_bits!(bitreader, 16),
            mapping: read_bits!(bitreader, 8),
        };

        if ret.window_type != 0 {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Bad window type: {}", ret.window_type)))
        } else if ret.transform_type != 0 {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Bad transfrom type: {}", ret.transform_type)))
        } else if ret.mapping as usize >= vorbis_info.maps.len() {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Mapping exceeded boundary: {} >= {}", ret.mapping, vorbis_info.maps.len())))
        } else {
            Ok(ret)
        }
    }
}

impl VorbisPackableObject for VorbisMode {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        write_bits!(bitwriter, if self.block_flag {1} else {0}, 1);
        write_bits!(bitwriter, self.window_type, 16);
        write_bits!(bitwriter, self.transform_type, 16);
        write_bits!(bitwriter, self.mapping, 8);

        Ok(bitwriter.total_bits - begin_bits)
    }
}

/// * The `VorbisSetupHeader` is the Vorbis setup header, the third header
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisSetupHeader {
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
    pub codebooks: Vec<CodeBook>,

    /// Encode only
    pub psys: CopiableBuffer<VorbisInfoPsy, 4>,
    pub psy_g: VorbisInfoPsyGlobal,

    pub halfrate_flag: bool,
}

impl VorbisSetupHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader, ident_header: &VorbisIdentificationHeader) -> Result<Self, io::Error> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x05vorbis" {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let mut ret = Self::default();

            let books = read_bits!(bitreader, 8).wrapping_add(1);
            if books == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No codebook backend settings.".to_string()));
            }
            for _ in 0..books {
                ret.static_codebooks.push(StaticCodeBook::load(bitreader)?);
            }

            // time backend settings; hooks are unused
            let times = read_bits!(bitreader, 6).wrapping_add(1);
            if times == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No time backend settings.".to_string()));
            }
            for _ in 0..times {
                let time_type = read_bits!(bitreader, 16);
                if time_type != 0 {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid time type {time_type}")));
                }
            }

            // floor backend settings
            let floors = read_bits!(bitreader, 6).wrapping_add(1);
            if floors == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No floor backend settings.".to_string()));
            }
            for _ in 0..floors {
                ret.floors.push(VorbisFloor::load(bitreader, &ret)?);
            }

            // residue backend settings
            let residues = read_bits!(bitreader, 6).wrapping_add(1);
            if residues == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No residues backend settings.".to_string()));
            }
            for _ in 0..residues {
                ret.residues.push(VorbisResidue::load(bitreader, &ret)?);
            }

            // map backend settings
            let maps = read_bits!(bitreader, 6).wrapping_add(1);
            if maps == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No map backend settings.".to_string()));
            }
            for _ in 0..maps {
                ret.maps.push(VorbisMapping::load(bitreader, &ret, ident_header)?);
            }

            // mode settings
            let modes = read_bits!(bitreader, 6).wrapping_add(1);
            if modes == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No mode settings.".to_string()));
            }
            for _ in 0..modes {
                ret.modes.push(VorbisMode::load(bitreader, &ret)?);
            }

            // EOP
            let end_of_packet = read_bits!(bitreader, 1) & 1 == 1;
            if !end_of_packet {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("End of packet flag == {end_of_packet}")));
            }

            Ok(ret)
        }
    }

    pub fn init_codebooks(&mut self, for_encode: bool) -> Result<(), io::Error> {
        self.codebooks.clear();
        self.codebooks.reserve(self.static_codebooks.len());
        for book in self.static_codebooks.iter() {
            self.codebooks.push(CodeBook::new(for_encode, book)?);
        }
        Ok(())
    }
}

impl VorbisPackableObject for VorbisSetupHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        write_slice!(bitwriter, b"\x05vorbis");

        // books
        write_bits!(bitwriter, self.static_codebooks.len().wrapping_sub(1), 8);
        for book in self.static_codebooks.iter() {
            book.pack(bitwriter)?;
        }

        // times
        write_bits!(bitwriter, 0, 6);
        write_bits!(bitwriter, 0, 16);

        // floors
        write_bits!(bitwriter, self.floors.len().wrapping_sub(1), 6);
        for floor in self.floors.iter() {
            floor.pack(bitwriter)?;
        }

        // residues
        write_bits!(bitwriter, self.residues.len().wrapping_sub(1), 6);
        for residue in self.residues.iter() {
            residue.pack(bitwriter)?;
        }

        // maps
        write_bits!(bitwriter, self.maps.len().wrapping_sub(1), 6);
        for map in self.maps.iter() {
            map.pack(bitwriter)?;
        }

        // modes
        write_bits!(bitwriter, self.modes.len().wrapping_sub(1), 6);
        for mode in self.modes.iter() {
            mode.pack(bitwriter)?;
        }

        // EOP
        write_bits!(bitwriter, 1, 1);

        Ok(bitwriter.total_bits - begin_bits)
    }
}

/// * The `VorbisInfo` structure
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisInfo {
    pub version: i32,
    pub channels: i32,
    pub sample_rate: i32,
    pub bitrate_upper: i32,
    pub bitrate_nominal: i32,
    pub bitrate_lower: i32,
    pub bitrate_window: i32,
    pub block_size: [i32; 2],
    pub codec_setup: VorbisSetupHeader,
}

impl VorbisInfo {
    pub fn new(identification_header: &VorbisIdentificationHeader, setup_header: &VorbisSetupHeader) -> Self {
        Self {
            version: identification_header.version,
            channels: identification_header.channels,
            sample_rate: identification_header.sample_rate,
            bitrate_upper: identification_header.bitrate_upper,
            bitrate_nominal: identification_header.bitrate_nominal,
            bitrate_lower: identification_header.bitrate_lower,
            bitrate_window: 0,
            block_size: identification_header.block_size,
            codec_setup: setup_header.clone()
        }
    }
}

/// * The private part of the `VorbisDspState` for `libvorbis-1.3.7`
#[derive(Debug, Default, Clone, PartialEq)]
struct VorbisDspStatePrivate {
    envelope: Option<VorbisEnvelopeLookup>,
    window: [i32; 2],
    transform: [MdctLookup; 2],
    fft_look: [DrftLookup; 2],
    modebits: i32,
}

impl VorbisDspStatePrivate{
    /// Analysis side code, but directly related to blocking. Thus it's
    /// here and not in analysis.c (which is for analysis transforms only).
    /// The init is here because some of it is shared
    pub fn new(info: &mut VorbisInfo, for_encode: bool) -> Result<Self, io::Error> {
        let codec_info = &info.codec_setup;
        let block_size = [info.block_size[0] as usize, info.block_size[1] as usize];
        let hs = if codec_info.halfrate_flag {1} else {0};

        assert!(codec_info.modes.len() > 0);
        assert!(block_size[0] >= 64);
        assert!(block_size[1] >= block_size[0]);

        let mut ret = Self {
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
            ..Default::default()
        };

        info.codec_setup.init_codebooks(for_encode)?;
        if for_encode {
            ret.fft_look = [
                DrftLookup::new(block_size[0]),
                DrftLookup::new(block_size[1]),
            ];
        }

        Ok(ret)
    }
}

/// * Am I going to reinvent the `libvorbis` wheel myself?
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisDspState {
    pub info: VorbisInfo,
    backend_state: VorbisDspStatePrivate,
    for_encode: bool,
}

#[derive(Default, Debug, Clone, PartialEq)]
struct VorbisBlockInternal {
    pcmdelay: Vec<Vec<f32>>,
    ampmax: f32,
    blocktype: i32,
}

/// Necessary stream state for linking to the framing abstraction
#[derive(Default, Debug, Clone, PartialEq)]
pub struct VorbisBlock<'a> {
    pcm: Vec<Vec<f32>>,

    lw: i32,
    w: i32,
    nw: i32,
    pcmend: i32,
    mode: i32,

    eofflag: bool,
    granulepos: i64,
    sequence: i64,

    /// For read-only access of configuration
    vorbis_dsp_state: &'a VorbisDspState,

    glue_bits: i32,
    time_bits: i32,
    floor_bits: i32,
    res_bits: i32,

    internal: VorbisBlockInternal,
}

