#![allow(dead_code)]
use std::io::{self, Write};

use crate::*;

use ogg::OggPacket;
use io_utils::CursorVecU8;
use bitwise::{BitReader, BitWriter};
use codebook::StaticCodeBook;
use floor::VorbisFloor;
use residue::VorbisResidue;
use mapping::VorbisMapping;

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
    pub fn load(bitreader: &mut BitReader) -> io::Result<Self> {
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
    pub fn load_from_slice(data: &[u8]) -> io::Result<Self> {
        let mut bitreader = BitReader::new(data);
        Self::load(&mut bitreader)
    }

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> io::Result<usize>
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
    pub fn load(bitreader: &mut BitReader) -> io::Result<Self> {
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

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> io::Result<usize>
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> io::Result<Self> {
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

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> io::Result<usize>
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
}

impl VorbisSetupHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader, ident_header: &VorbisIdentificationHeader) -> io::Result<Self> {
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

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>, ident_header: &VorbisIdentificationHeader) -> io::Result<usize>
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
            map.pack(bitwriter, ident_header.channels)?;
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

/// * This function extracts data from some Ogg packets, the packets contains the Vorbis headers.
/// * There are 3 kinds of Vorbis headers, they are the identification header, the metadata header, and the setup header.
#[allow(clippy::type_complexity)]
pub fn get_vorbis_headers_from_ogg_packet_bytes(data: &[u8], stream_id: &mut u32) -> io::Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let mut cursor = CursorVecU8::new(data.to_vec());
    let ogg_packets = OggPacket::from_cursor(&mut cursor);

    let mut ident_header = Vec::<u8>::new();
    let mut metadata_header = Vec::<u8>::new();
    let mut setup_header = Vec::<u8>::new();

    // Parse the body of the Ogg Stream.
    // The body consists of a table and segments of data. The table describes the length of each segment of data
    // The Vorbis header must occur at the beginning of a segment
    // And if the header is long enough, it crosses multiple segments
    let mut cur_segment_type = 0;
    for packet in ogg_packets.iter() {
        for segment in packet.get_segments().iter() {
            if segment[1..7] == *b"vorbis" && [1, 3, 5].contains(&segment[0]) {
                cur_segment_type = segment[0];
            } // Otherwise it's not a Vorbis header
            match cur_segment_type {
                1 => ident_header.extend(segment),
                3 => metadata_header.extend(segment),
                5 => setup_header.extend(segment),
                o => return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid Vorbis header type {o}"))),
            }
        }
    }

    *stream_id = ogg_packets[0].stream_id;
    Ok((ident_header, metadata_header, setup_header))
}

/// * This function extracts data from Ogg packets, the packets contains the Vorbis header.
/// * The packets were all decoded.
pub fn parse_vorbis_headers(data: &[u8], stream_id: &mut u32) -> (VorbisIdentificationHeader, VorbisCommentHeader, VorbisSetupHeader) {
    let (b1, b2, b3) = get_vorbis_headers_from_ogg_packet_bytes(data, stream_id).unwrap();
    debugln!("b1 = [{}]", format_array!(b1, " ", "{:02x}"));
    debugln!("b2 = [{}]", format_array!(b2, " ", "{:02x}"));
    debugln!("b3 = [{}]", format_array!(b3, " ", "{:02x}"));
    let mut br1 = BitReader::new(&b1);
    let mut br2 = BitReader::new(&b2);
    let mut br3 = BitReader::new(&b3);
    let h1 = VorbisIdentificationHeader::load(&mut br1).unwrap();
    let h2 = VorbisCommentHeader::load(&mut br2).unwrap();
    let h3 = VorbisSetupHeader::load(&mut br3, &h1).unwrap();
    (h1, h2, h3)
}