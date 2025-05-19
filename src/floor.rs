use std::{
    fmt::{self, Debug, Formatter},
    io::{self, Write},
};

use crate::*;
use utils::*;
use headers::VorbisSetupHeader;
use copiablebuf::CopiableBuffer;

const VIF_POSIT: usize = 63;
const VIF_CLASS: usize = 16;
const VIF_PARTS: usize = 31;

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

    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        match self {
            Self::Floor0(floor0) => floor0.pack(bitwriter),
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

#[derive(Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisLookFloor0<'a> {
    ln: i32,
    m: i32,
    linearmap: Vec<Vec<i32>>,
    n: [i32; 2],

    info: &'a VorbisFloor0,

    bits: i32,
    frames: i32,
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

    /// * Pack to the bitstream
    pub fn pack<W>(&self, _: &mut BitWriter<W>) -> Result<usize, io::Error>
    where
        W: Write {
        // Floor0 never pack.
        Ok(0)
    }

    pub fn look(&self) -> VorbisLookFloor0 {
        VorbisLookFloor0 {
            ln: self.barkmap,
            m: self.order,
            linearmap: Vec::new(),
            info: &self,
            ..Default::default()
        }
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
        .field("books", &format_args!("[{}]", format_array!(self.books)))
        .field("lessthan", &self.lessthan)
        .field("greaterthan", &self.greaterthan)
        .finish()
    }
}

impl Debug for VorbisLookFloor0<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisLookFloor0")
        .field("ln", &self.ln)
        .field("m", &self.m)
        .field("linearmap", &NestVecFormatter::new_level1(&self.linearmap))
        .field("n", &self.n)
        .field("info", &self.info)
        .field("bits", &self.bits)
        .field("frames", &self.frames)
        .finish()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct VorbisFloor1 {
    /// 0 to 31
    pub partitions: i32,

    /// 0 to 15
    pub partitions_class: CopiableBuffer<i32, VIF_PARTS>,

    /// 1 to 8
    pub class_dim: CopiableBuffer<i32, VIF_CLASS>,

    /// 0,1,2,3 (bits: 1<<n poss)
    pub class_subs: CopiableBuffer<i32, VIF_CLASS>,

    /// subs ^ dim entries
    pub class_book: CopiableBuffer<i32, VIF_CLASS>,

    /// [VIF_CLASS][subs]
    pub class_subbook: CopiableBuffer<CopiableBuffer<i32, 8>, VIF_CLASS>,

    /// 1 2 3 or 4
    pub mult: i32,

    /// first two implicit
    pub postlist: CopiableBuffer<i32, {VIF_POSIT + 2}>,

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
            if count > VIF_POSIT {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid class dim sum {count}, max is {VIF_POSIT}")));
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
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad postlist: [{}]", format_array!(ret.postlist))));
            }
        }

        Ok(VorbisFloor::Floor1(ret))
    }

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
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
        .field("partitions_class", &format_args!("[{}]", format_array!(self.partitions_class)))
        .field("class_dim", &format_args!("[{}]", format_array!(self.class_dim)))
        .field("class_subs", &format_args!("[{}]", format_array!(self.class_subs)))
        .field("class_book", &format_args!("[{}]", format_array!(self.class_book)))
        .field("class_subbook", &format_args!("[{}]", self.class_subbook.iter().map(|subbook|format!("[{}]", format_array!(subbook))).collect::<Vec<_>>().join(", ")))
        .field("mult", &self.mult)
        .field("postlist", &format_args!("[{}]", format_array!(self.postlist)))
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
