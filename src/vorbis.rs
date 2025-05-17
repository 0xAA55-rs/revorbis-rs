#![allow(dead_code)]
use std::{
    cmp::max,
    fmt::{self, Debug, Formatter},
    io::{self, Write},
    mem,
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};
use crate::bitwise::*;
use crate::mdct::MdctLookup;
use crate::envelope::VorbisEnvelopeLookup;

use ogg::{OggPacket, OggPacketType};
use bitwise::BitwiseData;
use io_utils::{Writer, CursorVecU8};
use copiablebuf::CopiableBuffer;

use mdct::MdctLookup;

const SHOW_DEBUG: bool = false;
const DEBUG_ON_READ_BITS: bool = false;
const DEBUG_ON_WRITE_BITS: bool = false;
const PANIC_ON_ERROR: bool = false;
/// * This is the parsed Vorbis codebook, it's used to quantify the audio samples.
/// * This is the re-invented wheel. For this piece of code, this thing is only used to parse the binary form of the codebooks.
/// * And then I can sum up how many **bits** were used to store the codebooks.
/// * Vorbis data are all stored in bitwise form, almost anything is not byte-aligned. Split data in byte arrays just won't work on Vorbis data.
/// * We have to do it in a bitwise way.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct CodeBook {
    pub dim: i32,
    pub entries: i32,
    pub lengthlist: Vec<i8>,
    pub maptype: i32,
    pub q_min: i32,
    pub q_delta: i32,
    pub q_quant: i32,
    pub q_sequencep: i32,
    pub quantlist: Vec<i32>,
}

impl Debug for CodeBook {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("CodeBook")
        .field("dim", &self.dim)
        .field("entries", &self.entries)
        .field("lengthlist", &format_args!("[{}]", format_array!(self.lengthlist, ", ", "{}")))
        .field("maptype", &self.maptype)
        .field("q_min", &self.q_min)
        .field("q_delta", &self.q_delta)
        .field("q_quant", &self.q_quant)
        .field("q_sequencep", &self.q_sequencep)
        .field("quantlist", &format_args!("[{}]", format_array!(self.quantlist, ", ", "{}")))
        .finish()
    }
}

impl CodeBook {
    /// unpacks a codebook from the packet buffer into the codebook struct,
    /// readies the codebook auxiliary structures for decode
    pub fn load(bitreader: &mut BitReader) -> Result<Self, io::Error> {
        let mut ret = Self::default();

        /* make sure alignment is correct */
        if read_bits!(bitreader, 24) != 0x564342 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, "Check the `BCV` flag failed.".to_string()));
        }

        /* first the basic parameters */
        ret.dim = read_bits!(bitreader, 16);
        ret.entries = read_bits!(bitreader, 24);
        if ilog!(ret.dim) + ilog!(ret.entries) > 24 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("{} + {} > 24", ilog!(ret.dim), ilog!(ret.entries))));
        }

        /* codeword ordering.... length ordered or unordered? */
        match read_bits!(bitreader, 1) {
            0 => {
                /* allocated but unused entries? */
                let unused = read_bits!(bitreader, 1) != 0;

                /* unordered */
                ret.lengthlist.resize(ret.entries as usize, 0);

                /* allocated but unused entries? */
                if unused {
                    /* yes, unused entries */
                    for i in 0..ret.entries as usize {
                        if read_bits!(bitreader, 1) != 0 {
                            let num = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                            ret.lengthlist[i] = num;
                        } else {
                            ret.lengthlist[i] = 0;
                        }
                    }
                } else { /* all entries used; no tagging */
                    for i in 0..ret.entries as usize {
                        let num = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                        ret.lengthlist[i] = num;
                    }
                }
            }
            1 => { /* ordered */
                let mut length = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                ret.lengthlist.resize(ret.entries as usize, 0);
                let mut i = 0;
                while i < ret.entries {
                    let num = read_bits!(bitreader, ilog!(ret.entries - i));
                    if length > 32 || num > ret.entries - i || (num > 0 && (num - 1) >> (length - 1) > 1) {
                        return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("length({length}) > 32 || num({num}) > entries({}) - i({i}) || (num({num}) > 0 && (num({num}) - 1) >> (length({length}) - 1) > 1)", ret.entries)));
                    }
                    for _ in 0..num {
                        ret.lengthlist[i as usize] = length;
                        i += 1;
                    }
                    length += 1;
                }
            }
            o => return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Unexpected codeword ordering {o}"))),
        }

        /* Do we have a mapping to unpack? */
        ret.maptype = read_bits!(bitreader, 4);
        match ret.maptype {
            0 => (),
            1 | 2 => {
                /* implicitly populated value mapping */
                /* explicitly populated value mapping */
                ret.q_min = read_bits!(bitreader, 32);
                ret.q_delta = read_bits!(bitreader, 32);
                ret.q_quant = read_bits!(bitreader, 4).wrapping_add(1);
                ret.q_sequencep = read_bits!(bitreader, 1);

                let quantvals = match ret.maptype {
                    1 => if ret.dim == 0 {0} else {ret.book_maptype1_quantvals() as usize},
                    2 => ret.entries as usize * ret.dim as usize,
                    _ => unreachable!(),
                };

                /* quantized values */
                ret.quantlist.resize(quantvals, 0);
                for i in 0..quantvals {
                    ret.quantlist[i] = read_bits!(bitreader, ret.q_quant);
                }
            }
            o => return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Unexpected maptype {o}"))),
        }
        Ok(ret)
    }

    /// there might be a straightforward one-line way to do the below
    /// that's portable and totally safe against roundoff, but I haven't
    /// thought of it.  Therefore, we opt on the side of caution
    fn book_maptype1_quantvals(&self) -> i32 {
        if self.entries < 1 {
            return 0;
        }
        let entries = self.entries as i32;
        let dim = self.dim as i32;
        let mut vals: i32 = (entries as f32).powf(1.0 / (dim as f32)).floor() as i32;
        /* the above *should* be reliable, but we'll not assume that FP is
           ever reliable when bitstream sync is at stake; verify via integer
           means that vals really is the greatest value of dim for which
           vals^b->bim <= b->entries */
        /* treat the above as an initial guess */
        vals = max(vals, 1);
        loop {
            let mut acc = 1i32;
            let mut acc1 = 1i32;
            let mut i = 0i32;
            while i < dim {
                if entries / vals < acc {
                    break;
                }
                acc *= vals;
                if i32::MAX / (vals + 1) < acc1 {
                    acc1 = i32::MAX;
                } else {
                    acc1 *= vals + 1;
                }
                i += 1;
            }
            if i >= dim && acc <= entries && acc1 > entries {
                return vals;
            } else if i < dim || acc > entries {
                vals -= 1;
            } else {
                vals += 1;
            }
        }
    }
}

impl VorbisPackableObject for CodeBook {
    /// * Pack the book into the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        /* first the basic parameters */
        write_bits!(bitwriter, 0x564342, 24);
        write_bits!(bitwriter, self.dim, 16);
        write_bits!(bitwriter, self.entries, 24);

        /* pack the codewords.  There are two packings; length ordered and
           length random.  Decide between the two now. */

        let mut ordered = false;
        let mut i = 1usize;
        while i < self.entries as usize {
            if self.lengthlist[i - 1] == 0 || self.lengthlist[i] < self.lengthlist[i - 1] {
                break;
            }
            i += 1;
        }
        if i == self.entries as usize {
            ordered = true;
        }

        if ordered {
            /* length ordered.  We only need to say how many codewords of
               each length.  The actual codewords are generated
               deterministically */
            let mut count = 0i32;
            write_bits!(bitwriter, 1, 1); /* ordered */
            write_bits!(bitwriter, self.lengthlist[0].wrapping_sub(1), 5);

            for i in 1..self.entries as usize {
                let this = self.lengthlist[i];
                let last = self.lengthlist[i - 1];
                if this > last {
                    for _ in last..this {
                        write_bits!(bitwriter, i as i32 - count, ilog!(self.entries - count));
                        count = i as i32;
                    }
                }
            }
            write_bits!(bitwriter, self.entries - count, ilog!(self.entries - count));
        } else {
            /* length random.  Again, we don't code the codeword itself, just
               the length.  This time, though, we have to encode each length */
            write_bits!(bitwriter, 0, 1); /* unordered */

            /* algortihmic mapping has use for 'unused entries', which we tag
               here.  The algorithmic mapping happens as usual, but the unused
               entry has no codeword. */
            let mut i = 0i32;
            while i < self.entries {
                if self.lengthlist[i as usize] == 0 {
                    break;
                }
                i += 1;
            }

            if i == self.entries {
                write_bits!(bitwriter, 0, 1); /* no unused entries */
                for i in 0..self.entries as usize {
                    write_bits!(bitwriter, self.lengthlist[i].wrapping_sub(1), 5);
                }
            } else {
                write_bits!(bitwriter, 1, 1); /* we have unused entries; thus we tag */
                for i in 0..self.entries as usize {
                    if self.lengthlist[i] == 0 {
                        write_bits!(bitwriter, 0, 1);
                    } else {
                        write_bits!(bitwriter, 1, 1);
                        write_bits!(bitwriter, self.lengthlist[i].wrapping_sub(1), 5);
                    }
                }
            }
        }

        /* is the entry number the desired return value, or do we have a
           mapping? If we have a mapping, what type? */
        write_bits!(bitwriter, self.maptype, 4);
        match self.maptype {
            0 => (),
            1 | 2 => {
                if self.quantlist.is_empty() {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, "Missing quantlist data".to_string()));
                }

                write_bits!(bitwriter, self.q_min, 32);
                write_bits!(bitwriter, self.q_delta, 32);
                write_bits!(bitwriter, self.q_quant.wrapping_sub(1), 4);
                write_bits!(bitwriter, self.q_sequencep, 1);

                let quantvals = match self.maptype {
                    1 => self.book_maptype1_quantvals() as usize,
                    2 => self.entries as usize * self.dim as usize,
                    _ => unreachable!(),
                };

                for i in 0..quantvals {
                    write_bits!(bitwriter, self.quantlist[i].unsigned_abs(), self.q_quant);
                }
            }
            o => return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Unexpected maptype {o}"))),
        }

        Ok(bitwriter.total_bits - begin_bits)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CodeBooksPacked {
    /// * The packed code books
    pub books: BitwiseData,

    /// * The size of each codebook in bits
    pub bits_of_books: Vec<usize>,
}

impl CodeBooksPacked {
    pub fn unpack(&self) -> Result<CodeBooks, io::Error> {
        CodeBooks::load_from_slice(&self.books.data)
    }

    /// * Get the number of total bits in the `data` field
    pub fn get_total_bits(&self) -> usize {
        self.books.get_total_bits()
    }

    /// * Get the number of bytes that are just enough to contain all of the bits.
    pub fn get_total_bytes(&self) -> usize {
        self.books.get_total_bytes()
    }

    /// * Resize to the aligned size. Doing this is for `shift_data_to_front()` and `shift_data_to_back()` to manipulate bits efficiently.
    pub fn fit_to_aligned_size(&mut self) {
        self.books.fit_to_aligned_size()
    }

    /// * Resize to the number of bytes that are just enough to contain all of the bits.
    pub fn shrink_to_fit(&mut self) {
        self.books.shrink_to_fit()
    }

    /// * Check if the data length is just the aligned size.
    pub fn is_aligned_size(&self) -> bool {
        self.books.is_aligned_size()
    }

    /// * Breakdown to each book
    pub fn split(&self) -> Vec<BitwiseData> {
        let num_books = self.bits_of_books.len();
        if num_books == 0 {
            return Vec::new();
        }
        let mut ret = Vec::<BitwiseData>::with_capacity(num_books);
        let mut books = BitwiseData {
            data: self.books.data[1..].to_vec(),
            total_bits: self.books.total_bits - 8,
        };
        for i in 0..num_books {
            let cur_book_bits = self.bits_of_books[i];
            let (front, back) = books.split(cur_book_bits);
            ret.push(front);
            books = back;
        }
        ret
    }

    /// * Concat a packed book without a gap
    pub fn concat(&mut self, book: &BitwiseData) {
        self.books.concat(book);
        self.bits_of_books.push(book.total_bits);
    }

    /// * Turn to byte array
    pub fn into_bytes(self) -> Vec<u8> {
        self.books.into_bytes()
    }
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct CodeBooks {
    /// * The unpacked codebooks
    pub books: Vec<CodeBook>,

    /// * The size of each codebook in bits if they are packed
    pub bits_of_books: Vec<usize>,

    /// * The total bits of all the books
    pub total_bits: usize,
}

impl CodeBooks {
    /// * Unpack the codebooks from the bitstream
    pub fn load(bitreader: &mut BitReader) -> Result<Self, io::Error> {
        let begin_bits = bitreader.total_bits;
        let num_books = (read_bits!(bitreader, 8).wrapping_add(1)) as usize;
        let mut books = Vec::<CodeBook>::with_capacity(num_books);
        let mut bits_of_books = Vec::<usize>::with_capacity(num_books);
        for _ in 0..num_books {
            let cur_bit_pos = bitreader.total_bits;
            books.push(CodeBook::load(bitreader)?);
            bits_of_books.push(bitreader.total_bits - cur_bit_pos);
        }
        Ok(Self {
            books,
            bits_of_books,
            total_bits: bitreader.total_bits - begin_bits,
        })
    }

    /// * Unpack from a slice
    pub fn load_from_slice(data: &[u8]) -> Result<Self, io::Error> {
        let mut bitreader = BitReader::new(data);
        Self::load(&mut bitreader)
    }

    /// * Get the total bits of the codebook.
    pub fn get_total_bits(&self) -> usize {
        self.total_bits
    }

    /// * Get the total bytes of the codebook that are able to contain all of the bits.
    pub fn get_total_bytes(&self) -> usize {
        BitwiseData::calc_total_bytes(self.total_bits)
    }

    /// * Get how many books
    pub fn len(&self) -> usize {
        self.books.len()
    }

    /// * Get is empty
    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// * Pack the codebook to binary for storage.
    pub fn to_packed_codebooks(&self) -> Result<CodeBooksPacked, io::Error> {
        let mut bitwriter = BitWriter::new(CursorVecU8::default());
        let mut bits_of_books = Vec::<usize>::with_capacity(self.books.len());
        write_bits!(bitwriter, self.books.len().wrapping_sub(1), 8);
        for book in self.books.iter() {
            let cur_bit_pos = bitwriter.total_bits;
            book.pack(&mut bitwriter)?;
            bits_of_books.push(bitwriter.total_bits - cur_bit_pos);
        }
        let total_bits = bitwriter.total_bits;
        let books = bitwriter.into_bytes();
        Ok(CodeBooksPacked{
            books: BitwiseData::new(&books, total_bits),
            bits_of_books,
        })
    }
}

impl VorbisPackableObject for CodeBooks {
    /// * Pack to bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        write_bits!(bitwriter, self.books.len().wrapping_sub(1), 8);
        for book in self.books.iter() {
            book.pack(bitwriter)?;
        }
        let total_bits = bitwriter.total_bits - begin_bits;
        assert_eq!(total_bits, self.total_bits);
        Ok(total_bits)
    }
}

impl From<CodeBooksPacked> for CodeBooks {
    fn from(packed: CodeBooksPacked) -> Self {
        let ret = Self::load_from_slice(&packed.books.data).unwrap();
        assert_eq!(ret.bits_of_books, packed.bits_of_books, "CodeBooks::from(&CodeBooksPacked), bits_of_books");
        assert_eq!(ret.total_bits, packed.books.total_bits, "CodeBooks::from(&CodeBooksPacked), total_bits");
        ret
    }
}

impl Debug for CodeBooks {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("CodeBooks")
        .field("books", &self.books)
        .field("bits_of_books", &format_args!("[{}]", format_array!(self.bits_of_books, ", ", "0x{:04x}")))
        .field("total_bits", &self.total_bits)
        .finish()
    }
}

derive_index!(CodeBooks, CodeBook, books);

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

/// * The `VorbisFloor` for floor types
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum VorbisFloor {
    Floor0(VorbisFloor0),
    Floor1(VorbisFloor1),
}

impl VorbisFloor {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, io::Error> {
        let floor_type = read_bits!(bitreader, 16);
        match floor_type {
            0 => Ok(VorbisFloor0::load(bitreader, vorbis_info)?),
            1 => Ok(VorbisFloor1::load(bitreader, vorbis_info)?),
            o => Err(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid floor type {o}"))),
        }
    }

    pub fn get_type(&self) -> u16 {
        match self {
            Self::Floor0(_) => 0,
            Self::Floor1(_) => 1,
        }
    }
}

impl VorbisPackableObject for VorbisFloor {
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        match self {
            Self::Floor0(_) => Ok(0),
            Self::Floor1(floor1) => floor1.pack(bitwriter),
        }
    }
}

impl Default for VorbisFloor {
    fn default() -> Self {
        Self::Floor0(VorbisFloor0::default())
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisFloor0 {
    pub order: i32,
    pub rate: i32,
    pub barkmap: i32,
    pub ampbits: i32,
    pub ampdB: i32,
    pub books: CopiableBuffer<i32, 16>,

    /// encode-only config setting hacks for libvorbis
    pub lessthan: f32,

    /// encode-only config setting hacks for libvorbis
    pub greaterthan: f32,
}

impl VorbisFloor0 {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, io::Error> {
        let static_codebooks = &vorbis_info.static_codebooks;
        let mut ret = Self {
            order: read_bits!(bitreader, 8),
            rate: read_bits!(bitreader, 16),
            barkmap: read_bits!(bitreader, 16),
            ampbits: read_bits!(bitreader, 8),
            ampdB: read_bits!(bitreader, 8),
            ..Default::default()
        };

        let num_books = read_bits!(bitreader, 4).wrapping_add(1) as usize;
        if ret.order < 1
        || ret.rate < 1
        || ret.barkmap < 1
        || num_books < 1 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid floor 0 data: \norder = {}\nrate = {}\nbarkmap = {}\nnum_books = {num_books}",
                ret.order,
                ret.rate,
                ret.barkmap
            )));
        }

        for _ in 0..num_books {
            let book = read_bits!(bitreader, 8);
            if book < 0 || book as usize >= static_codebooks.len() {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid book number: {book}")));
            }
            if static_codebooks[book as usize].maptype == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "Invalid book maptype: 0".to_string()));
            }
            if static_codebooks[book as usize].dim < 1 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "Invalid book dimension: 0".to_string()));
            }
            ret.books.push(book);
        }

        Ok(VorbisFloor::Floor0(ret))
    }
}

impl Debug for VorbisFloor0 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisFloor0")
        .field("order", &self.order)
        .field("rate", &self.rate)
        .field("barkmap", &self.barkmap)
        .field("ampbits", &self.ampbits)
        .field("ampdB", &self.ampdB)
        .field("books", &format_args!("[{}]", format_array!(self.books, ", ", "{}")))
        .field("lessthan", &self.lessthan)
        .field("greaterthan", &self.greaterthan)
        .finish()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct VorbisFloor1 {
    /// 0 to 31
    pub partitions: i32,

    /// 0 to 15
    pub partitions_class: CopiableBuffer<i32, 31>,

    /// 1 to 8
    pub class_dim: CopiableBuffer<i32, 16>,

    /// 0,1,2,3 (bits: 1<<n poss)
    pub class_subs: CopiableBuffer<i32, 16>,

    /// subs ^ dim entries
    pub class_book: CopiableBuffer<i32, 16>,

    /// [VIF_CLASS][subs]
    pub class_subbook: CopiableBuffer<CopiableBuffer<i32, 8>, 16>,

    /// 1 2 3 or 4
    pub mult: i32,

    /// first two implicit
    pub postlist: CopiableBuffer<i32, 65>,

    /// encode side analysis parameters
    pub maxover: f32,

    /// encode side analysis parameters
    pub maxunder: f32,

    /// encode side analysis parameters
    pub maxerr: f32,

    /// encode side analysis parameters
    pub twofitweight: f32,

    /// encode side analysis parameters
    pub twofitatten: f32,

    pub n: i32,
}

impl VorbisFloor1 {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, io::Error> {
        let static_codebooks = &vorbis_info.static_codebooks;
        let mut ret = Self::default();

        ret.partitions = read_bits!(bitreader, 5);
        ret.partitions_class.resize(ret.partitions as usize, 0);
        for i in 0..ret.partitions_class.len() {
            ret.partitions_class[i] = read_bits!(bitreader, 4);
        }
        let maxclass = ret.partitions_class.iter().copied().max().unwrap() as usize + 1;
        ret.class_dim.resize(maxclass, 0);
        ret.class_subs.resize(maxclass, 0);
        ret.class_book.resize(maxclass, 0);
        ret.class_subbook.resize(maxclass, CopiableBuffer::default());

        for i in 0..maxclass {
            ret.class_dim[i] = read_bits!(bitreader, 3).wrapping_add(1);
            ret.class_subs[i] = read_bits!(bitreader, 2);
            if ret.class_subs[i] != 0 {
                ret.class_book[i] = read_bits!(bitreader, 8);
            }
            if ret.class_book[i] as usize >= static_codebooks.len() {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid class book index {}, max books is {}", ret.class_book[i], static_codebooks.len())));
            }
            let sublen = 1usize << ret.class_subs[i];
            ret.class_subbook[i].resize(sublen, 0);
            for k in 0..sublen {
                let subbook_index = read_bits!(bitreader, 8).wrapping_sub(1);
                if subbook_index < -1 || subbook_index >= static_codebooks.len() as i32 {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid class subbook index {subbook_index}, max books is {}", static_codebooks.len())));
                }
                ret.class_subbook[i][k] = subbook_index;
            }
        }

        ret.mult = read_bits!(bitreader, 2).wrapping_add(1);
        let rangebits = read_bits!(bitreader, 4);
        let maxrange = 1 << rangebits;

        let mut k = 0usize;
        let mut count = 0usize;
        for i in 0..ret.partitions_class.len() {
            count += ret.class_dim[ret.partitions_class[i] as usize] as usize;
            if count > 63 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid class dim sum {count}, max is 63")));
            }
            ret.postlist.resize(count + 2, 0);
            while k < count {
                let t = read_bits!(bitreader, rangebits);
                if t < 0 || t >= maxrange {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid value for postlist {t}")));
                }
                ret.postlist[k + 2] = t;
                k += 1;
            }
        }
        ret.postlist[0] = 0;
        ret.postlist[1] = maxrange;

        let mut checker = ret.postlist[..].to_vec();
        checker.sort();
        for i in 1..checker.len() {
            if checker[i - 1] == checker[i] {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad postlist: [{}]", format_array!(ret.postlist, ", ", "{}"))));
            }
        }

        Ok(VorbisFloor::Floor1(ret))
    }
}

impl VorbisPackableObject for VorbisFloor1 {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        let maxposit = self.postlist[1];
        let rangebits = ilog!(maxposit - 1);
        // floor type
        write_bits!(bitwriter, 1, 16);
        write_bits!(bitwriter, self.partitions, 5);
        for i in 0..self.partitions_class.len() {
            write_bits!(bitwriter, self.partitions_class[i], 4);
        }
        let maxclass = self.partitions_class.iter().copied().max().unwrap() as usize + 1;
        for i in 0..maxclass {
            write_bits!(bitwriter, self.class_dim[i].wrapping_sub(1), 3);
            write_bits!(bitwriter, self.class_subs[i], 2);
            if self.class_subs[i] != 0 {
                write_bits!(bitwriter, self.class_book[i], 8);
            }
            for k in 0..self.class_subbook[i].len() {
                write_bits!(bitwriter, self.class_subbook[i][k].wrapping_add(1), 8);
            }
        }
        write_bits!(bitwriter, self.mult.wrapping_sub(1), 2);
        write_bits!(bitwriter, rangebits, 4);
        let mut k = 0usize;
        let mut count = 0usize;
        for i in 0..self.partitions_class.len() {
            count += self.class_dim[self.partitions_class[i] as usize] as usize;
            while k < count {
                write_bits!(bitwriter, self.postlist[k + 2], rangebits);
                k += 1;
            }
        }
        Ok(bitwriter.total_bits - begin_bits)
    }
}

impl Debug for VorbisFloor1 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisFloor1")
        .field("partitions", &self.partitions)
        .field("partitions_class", &format_args!("[{}]", format_array!(self.partitions_class, ", ", "{}")))
        .field("class_dim", &format_args!("[{}]", format_array!(self.class_dim, ", ", "{}")))
        .field("class_subs", &format_args!("[{}]", format_array!(self.class_subs, ", ", "{}")))
        .field("class_book", &format_args!("[{}]", format_array!(self.class_book, ", ", "{}")))
        .field("class_subbook", &format_args!("[{}]", self.class_subbook.iter().map(|subbook|format!("[{}]", format_array!(subbook, ", ", "{}"))).collect::<Vec<_>>().join(", ")))
        .field("mult", &self.mult)
        .field("postlist", &format_args!("[{}]", format_array!(self.postlist, ", ", "{}")))
        .field("maxover", &self.maxover)
        .field("maxunder", &self.maxunder)
        .field("maxerr", &self.maxerr)
        .field("twofitweight", &self.twofitweight)
        .field("twofitatten", &self.twofitatten)
        .field("n", &self.n)
        .finish()
    }
}

impl Default for VorbisFloor1 {
    fn default() -> Self {
        Self {
            partitions: 0,
            partitions_class: CopiableBuffer::default(),
            class_dim: CopiableBuffer::default(),
            class_subs: CopiableBuffer::default(),
            class_book: CopiableBuffer::default(),
            class_subbook: CopiableBuffer::default(),
            mult: 0,
            postlist: CopiableBuffer::default(),
            maxover: 0.0,
            maxunder: 0.0,
            maxerr: 0.0,
            twofitweight: 0.0,
            twofitatten: 0.0,
            n: 0
        }
    }
}

/// * block-partitioned VQ coded straight residue
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VorbisResidue {
    /// The residue type
    pub residue_type: i32,

    pub begin: i32,
    pub end: i32,

    /// group n vectors per partition
    pub grouping: i32,

    /// possible codebooks for a partition
    pub partitions: i32,

    /// partitions ^ groupbook dim
    pub partvals: i32,

    /// huffbook for partitioning
    pub groupbook: i32,

    /// expanded out to pointers in lookup
    pub secondstages: CopiableBuffer<i32, 64>,

    /// list of second stage books
    pub booklist: CopiableBuffer<i32, 512>,
}

impl VorbisResidue {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<Self, io::Error> {
        let static_codebooks = &vorbis_info.static_codebooks;
        let residue_type = read_bits!(bitreader, 16);

        if !(0..3).contains(&residue_type) {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid residue type {residue_type}")))
        }

        let mut ret = Self {
            residue_type,
            begin: read_bits!(bitreader, 24),
            end: read_bits!(bitreader, 24),
            grouping: read_bits!(bitreader, 24).wrapping_add(1),
            partitions: read_bits!(bitreader, 6).wrapping_add(1),
            groupbook: read_bits!(bitreader, 8),
            ..Default::default()
        };

        if !(0..static_codebooks.len()).contains(&(ret.groupbook as usize)) {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid groupbook index {}", ret.groupbook)));
        }

        let partitions = ret.partitions as usize;
        ret.secondstages.resize(partitions, 0);

        let mut acc = 0usize;
        for i in 0..partitions {
            let mut cascade = read_bits!(bitreader, 3);
            let cflag = read_bits!(bitreader, 1) != 0;
            if cflag {
                cascade |= read_bits!(bitreader, 5) << 3;
            }
            ret.secondstages[i] = cascade;
            acc += icount!(cascade);
        }

        ret.booklist.resize(acc, 0);
        for i in 0..acc {
            let book = read_bits!(bitreader, 8);
            if !(0..static_codebooks.len()).contains(&(book as usize)) {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid book index {book}")));
            }
            ret.booklist[i] = book;
            let book_maptype = static_codebooks[book as usize].maptype;
            if book_maptype == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid book maptype {book_maptype}")));
            }
        }

        let groupbook = &static_codebooks[ret.groupbook as usize];
        let entries = groupbook.entries;
        let mut dim = groupbook.dim;
        let mut partvals = 1i32;
        if dim < 1 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid groupbook dimension {dim}")));
        }
        while dim > 0 {
            partvals *= ret.partitions;
            if partvals > entries {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid partvals {partvals}")));
            }
            dim -= 1;
        }
        ret.partvals = partvals;
        Ok(ret)
    }
}

impl VorbisPackableObject for VorbisResidue {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        let mut acc = 0usize;

        write_bits!(bitwriter, self.residue_type, 16);
        write_bits!(bitwriter, self.begin, 24);
        write_bits!(bitwriter, self.end, 24);
        write_bits!(bitwriter, self.grouping.wrapping_sub(1), 24);
        write_bits!(bitwriter, self.partitions.wrapping_sub(1), 6);
        write_bits!(bitwriter, self.groupbook, 8);
        for i in 0..self.secondstages.len() {
            let secondstage = self.secondstages[i];
            if ilog!(secondstage) > 3 {
                write_bits!(bitwriter, secondstage, 3);
                write_bits!(bitwriter, 1, 1);
                write_bits!(bitwriter, secondstage >> 3, 5);
            } else {
                write_bits!(bitwriter, secondstage, 4);
            }
            acc += icount!(secondstage);
        }
        for i in 0..acc {
            write_bits!(bitwriter, self.booklist[i], 8);
        }

        Ok(bitwriter.total_bits - begin_bits)
    }
}

impl Debug for VorbisResidue {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisResidue")
        .field("residue_type", &self.residue_type)
        .field("begin", &self.begin)
        .field("end", &self.end)
        .field("grouping", &self.grouping)
        .field("partitions", &self.partitions)
        .field("partvals", &self.partvals)
        .field("groupbook", &self.groupbook)
        .field("secondstages", &format_args!("[{}]", format_array!(self.secondstages, ", ", "{}")))
        .field("booklist", &format_args!("[{}]", format_array!(self.booklist, ", ", "{}")))
        .finish()
    }
}

impl Default for VorbisResidue {
    fn default() -> Self {
        Self {
            residue_type: 0,
            begin: 0,
            end: 0,
            grouping: 0,
            partitions: 0,
            partvals: 0,
            groupbook: 0,
            secondstages: CopiableBuffer::default(),
            booklist: CopiableBuffer::default(),
        }
    }
}

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

