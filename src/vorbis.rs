use std::{
    io::{self, Write},
    mem,
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};

use crate::*;

use codebook::CodeBooks;
use floor::VorbisFloor;
use residue::VorbisResidue;
use mdct::MdctLookup;

const SHOW_DEBUG: bool = false;
const DEBUG_ON_READ_BITS: bool = false;
const DEBUG_ON_WRITE_BITS: bool = false;
const PANIC_ON_ERROR: bool = false;

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VorbisMapping {
    /// Mapping type
    pub mapping_type: i32,

    /// Channels
    pub channels: i32,

    /// <= 16
    pub submaps: i32,

    /// up to 256 channels in a Vorbis stream
    pub chmuxlist: CopiableBuffer<i32, 256>,

    /// [mux] submap to floors
    pub floorsubmap: CopiableBuffer<i32, 16>,

    /// [mux] submap to residue
    pub residuesubmap: CopiableBuffer<i32, 16>,

    pub coupling_steps: i32,
    pub coupling_mag: CopiableBuffer<i32, 256>,
    pub coupling_ang: CopiableBuffer<i32, 256>,
}

impl VorbisMapping {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader, ident_header: &VorbisIdentificationHeader) -> Result<Self, io::Error> {
        let mapping_type = read_bits!(bitreader, 16);

        if mapping_type != 0 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid mapping type {mapping_type}")))
        }

        let channels = ident_header.channels as i32;
        let floors = vorbis_info.floors.len() as i32;
        let residues = vorbis_info.residues.len() as i32;
        let submaps = if read_bits!(bitreader, 1) != 0 {
            let submaps = read_bits!(bitreader, 4).wrapping_add(1);
            if submaps == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No submaps.".to_string()));
            }
            submaps
        } else {
            1
        };
        let coupling_steps = if read_bits!(bitreader, 1) != 0 {
            let coupling_steps = read_bits!(bitreader, 8).wrapping_add(1);
            if coupling_steps == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No coupling steps.".to_string()));
            }
            coupling_steps
        } else {
            0
        };
        let mut ret = Self {
            submaps,
            channels,
            coupling_steps,
            ..Default::default()
        };

        let submaps = submaps as usize;
        let channels = channels as usize;
        let coupling_steps = coupling_steps as usize;

        ret.coupling_mag.resize(coupling_steps, 0);
        ret.coupling_ang.resize(coupling_steps, 0);
        for i in 0..coupling_steps {
            let test_m = read_bits!(bitreader, ilog!(channels - 1));
            let test_a = read_bits!(bitreader, ilog!(channels - 1));
            ret.coupling_mag[i] = test_m;
            ret.coupling_ang[i] = test_a;
            if test_m == test_a
            || test_m >= channels as i32
            || test_a >= channels as i32 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad values for test_m = {test_m}, test_a = {test_a}, channels = {channels}")));
            }
        }

        let reserved = read_bits!(bitreader, 2);
        if reserved != 0 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Reserved value is {reserved}")));
        }

        if submaps > 1 {
            ret.chmuxlist.resize(channels, 0);
            for i in 0..channels {
                let chmux = read_bits!(bitreader, 4);
                if chmux >= submaps as i32 {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Chmux {chmux} >= submaps {submaps}")));
                }
                ret.chmuxlist[i] = chmux;
            }
        }
        ret.floorsubmap.resize(submaps, 0);
        ret.residuesubmap.resize(submaps, 0);
        for i in 0..submaps {
            let _unused_time_submap = read_bits!(bitreader, 8);
            let floorsubmap = read_bits!(bitreader, 8);
            if floorsubmap >= floors {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("floorsubmap {floorsubmap} >= floors {floors}")));
            }
            ret.floorsubmap[i] = floorsubmap;
            let residuesubmap = read_bits!(bitreader, 8);
            if residuesubmap >= residues {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("residuesubmap {residuesubmap} >= residues {residues}")));
            }
            ret.residuesubmap[i] = residuesubmap;
        }
        Ok(ret)
    }
}

impl VorbisPackableObject for VorbisMapping {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        write_bits!(bitwriter, self.mapping_type, 16);
        if self.submaps > 1 {
            write_bits!(bitwriter, 1, 1);
            write_bits!(bitwriter, self.submaps.wrapping_sub(1), 4);
        } else {
            write_bits!(bitwriter, 0, 1);
        }

        if self.coupling_steps > 0 {
            write_bits!(bitwriter, 1, 1);
            write_bits!(bitwriter, self.coupling_steps.wrapping_sub(1), 8);
            for i in 0..self.coupling_steps as usize {
                write_bits!(bitwriter, self.coupling_mag[i], ilog!(self.channels - 1));
                write_bits!(bitwriter, self.coupling_ang[i], ilog!(self.channels - 1));
            }
        } else {
            write_bits!(bitwriter, 0, 1);
        }

        write_bits!(bitwriter, 0, 2);

        if self.submaps > 1 {
            for i in 0..self.channels as usize {
                write_bits!(bitwriter, self.chmuxlist[i], 4);
            }
        }
        for i in 0..self.submaps as usize {
            write_bits!(bitwriter, 0, 8); // time submap unused
            write_bits!(bitwriter, self.floorsubmap[i], 8);
            write_bits!(bitwriter, self.residuesubmap[i], 8);
        }

        Ok(bitwriter.total_bits - begin_bits)
    }
}

impl Debug for VorbisMapping {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisMapping")
        .field("mapping_type", &self.mapping_type)
        .field("channels", &self.channels)
        .field("submaps", &self.submaps)
        .field("chmuxlist", &format_args!("[{}]", format_array!(self.chmuxlist, ", ", "{}")))
        .field("floorsubmap", &format_args!("[{}]", format_array!(self.floorsubmap, ", ", "{}")))
        .field("residuesubmap", &format_args!("[{}]", format_array!(self.residuesubmap, ", ", "{}")))
        .field("coupling_steps", &self.coupling_steps)
        .field("coupling_mag", &format_args!("[{}]", format_array!(self.coupling_mag, ", ", "{}")))
        .field("coupling_ang", &format_args!("[{}]", format_array!(self.coupling_ang, ", ", "{}")))
        .finish()
    }
}

impl Default for VorbisMapping {
    fn default() -> Self {
        Self {
            mapping_type: 0,
            channels: 0,
            submaps: 0,
            chmuxlist: CopiableBuffer::default(),
            floorsubmap: CopiableBuffer::default(),
            residuesubmap: CopiableBuffer::default(),
            coupling_steps: 0,
            coupling_mag: CopiableBuffer::default(),
            coupling_ang: CopiableBuffer::default(),
        }
    }
}

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
    pub static_codebooks: CodeBooks,
    pub floors: Vec<VorbisFloor>,
    pub residues: Vec<VorbisResidue>,
    pub maps: Vec<VorbisMapping>,
    pub modes: Vec<VorbisMode>,
}

impl VorbisSetupHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader, ident_header: &VorbisIdentificationHeader) -> Result<Self, io::Error> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x05vorbis" {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let mut ret = Self {
                // codebooks
                static_codebooks: CodeBooks::load(bitreader)?,
                ..Default::default()
            };

            // time backend settings; hooks are unused
            let times = read_bits!(bitreader, 6).wrapping_add(1);
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
}

impl VorbisPackableObject for VorbisSetupHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        write_slice!(bitwriter, b"\x05vorbis");

        // books
        self.static_codebooks.pack(bitwriter)?;

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
    pub identification: VorbisIdentificationHeader,
    pub codec_setup: VorbisSetupHeader,
}

impl VorbisInfo {
    pub fn new(identification_header: &VorbisIdentificationHeader, setup_header: &VorbisSetupHeader) -> Self {
        Self {
            identification: identification_header.clone(),
            codec_setup: setup_header.clone()
        }
    }
}

/// * The private part of the `VorbisDspState` for `libvorbis-1.3.7`
#[derive(Debug, Default, Clone, PartialEq)]
struct VorbisDspStatePrivate {

}


/// * Am I going to reinvent the `libvorbis` wheel myself?
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisDspState {
    pub info: VorbisInfo,
    backend_state: VorbisDspStatePrivate,
}

}






}

