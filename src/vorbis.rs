#![allow(dead_code)]
use std::{
    cmp::max,
    fmt::{self, Debug, Formatter},
    io::Write,
    mem,
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};
use crate::errors::{AudioReadError, AudioError, AudioWriteError};
use crate::format_array;
use crate::io_utils::{Writer, CursorVecU8};
use crate::utils::{BitwiseData, CopiableBuffer};

const MASK: [u32; 33] = [
    0x00000000,
    0x00000001, 0x00000003, 0x00000007, 0x0000000f,
    0x0000001f, 0x0000003f, 0x0000007f, 0x000000ff,
    0x000001ff, 0x000003ff, 0x000007ff, 0x00000fff,
    0x00001fff, 0x00003fff, 0x00007fff, 0x0000ffff,
    0x0001ffff, 0x0003ffff, 0x0007ffff, 0x000fffff,
    0x001fffff, 0x003fffff, 0x007fffff, 0x00ffffff,
    0x01ffffff, 0x03ffffff, 0x07ffffff, 0x0fffffff,
    0x1fffffff, 0x3fffffff, 0x7fffffff, 0xffffffff
];

const SHOW_DEBUG: bool = true;
const DEBUG_ON_READ_BITS: bool = true;
const DEBUG_ON_WRITE_BITS: bool = false;
macro_rules! debugln {
    () => {
        if SHOW_DEBUG {
            println!("");
        }
    };
    ($($arg:tt)*) => {
        if SHOW_DEBUG {
            println!($($arg)*);
        }
    };
}

macro_rules! derive_index {
    ($object:ident, $target:ident, $member:tt) => {
        impl Index<usize> for $object {
            type Output = $target;

            #[track_caller]
            fn index(&self, index: usize) -> &$target {
                &self.$member[index]
            }
        }

        impl IndexMut<usize> for $object {
            #[track_caller]
            fn index_mut(&mut self, index: usize) -> &mut $target {
                &mut self.$member[index]
            }
        }

        impl Index<Range<usize>> for $object {
            type Output = [$target];

            #[track_caller]
            fn index(&self, range: Range<usize>) -> &[$target] {
                &self.$member[range]
            }
        }

        impl IndexMut<Range<usize>> for $object {
            #[track_caller]
            fn index_mut(&mut self, range: Range<usize>) -> &mut [$target] {
                &mut self.$member[range]
            }
        }

        impl Index<RangeFrom<usize>> for $object {
            type Output = [$target];

            #[track_caller]
            fn index(&self, range: RangeFrom<usize>) -> &[$target] {
                &self.$member[range]
            }
        }

        impl IndexMut<RangeFrom<usize>> for $object {
            #[track_caller]
            fn index_mut(&mut self, range: RangeFrom<usize>) -> &mut [$target] {
                &mut self.$member[range]
            }
        }

        impl Index<RangeTo<usize>> for $object {
            type Output = [$target];

            #[track_caller]
            fn index(&self, range: RangeTo<usize>) -> &[$target] {
                &self.$member[range]
            }
        }

        impl IndexMut<RangeTo<usize>> for $object {
            #[track_caller]
            fn index_mut(&mut self, range: RangeTo<usize>) -> &mut [$target] {
                &mut self.$member[range]
            }
        }

        impl Index<RangeFull> for $object {
            type Output = [$target];

            #[track_caller]
            fn index(&self, _range: RangeFull) -> &[$target] {
                &self.$member[..]
            }
        }

        impl IndexMut<RangeFull> for $object {
            #[track_caller]
            fn index_mut(&mut self, _range: RangeFull) -> &mut [$target] {
                &mut self.$member[..]
            }
        }
    }
}

macro_rules! ilog {
    ($v:expr) => {
        {
            let mut ret = 0;
            let mut v = $v as u64;
            while v != 0 {
                v >>= 1;
                ret += 1;
            }
            ret
        }
    }
}

macro_rules! icount {
    ($v:expr) => {
        {
            let mut ret = 0usize;
            let mut v = $v as u64;
            while v != 0 {
                ret += (v as usize) & 1;
                v >>= 1;
            }
            ret
        }
    }
}

/// * BitReader: read vorbis data bit by bit
#[derive(Default)]
pub struct BitReader<'a> {
    /// * Currently ends at which bit in the last byte
    pub endbit: i32,

    /// * How many bits did we read in total
    pub total_bits: usize,

    /// * Borrowed a slice of data
    pub data: &'a [u8],

    /// * Current byte index
    pub cursor: usize,
}

impl<'a> BitReader<'a> {
    /// * `data` is decapsulated from the Ogg stream
    /// * `cursor` is the read position of the `BitReader`
    /// * Pass `data` as a slice that begins from the part you want to read,
    ///   Then you'll get the `cursor` to indicate how many bytes this part of data takes.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            endbit: 0,
            total_bits: 0,
            cursor: 0,
            data,
        }
    }

    /// * Read data bit by bit
    /// * bits <= 32
    pub fn read(&mut self, mut bits: i32) -> Result<i32, AudioReadError> {
        if !(0..=32).contains(&bits) {
            return Err(AudioReadError::InvalidArguments(format!("Invalid bit number: {bits}")));
        }
        let mut ret: i32;
        let m = MASK[bits as usize];
        let origbits = bits;
        let cursor = self.cursor;

        // Don't want it panic, and don't want an Option.
        let ptr_index = |mut index: usize| -> Result<u8, AudioReadError> {
            index += cursor;
            let eof_err = || -> AudioReadError {
                AudioReadError::UnexpectedEof(format!("UnexpectedEof when trying to read {origbits} bits from the input position 0x{:x}", index))
            };
            self.data.get(index).ok_or(eof_err()).copied()
        };

        bits += self.endbit;
        if bits == 0 {
            return Ok(0);
        }

        ret = (ptr_index(0)? as i32) >> self.endbit;
        if bits > 8 {
            ret |= (ptr_index(1)? as i32) << (8 - self.endbit);
            if bits > 16 {
                ret |= (ptr_index(2)? as i32) << (16 - self.endbit);
                if bits > 24 {
                    ret |= (ptr_index(3)? as i32) << (24 - self.endbit);
                    if bits > 32 && self.endbit != 0 {
                        ret |= (ptr_index(4)? as i32) << (32 - self.endbit);
                    }
                }
            }
        }
        ret &= m as i32;
        self.cursor += (bits / 8) as usize;
        self.endbit = bits & 7;
        self.total_bits += origbits as usize;
        Ok(ret)
    }
}

/// * BitWriter: write vorbis data bit by bit
#[derive(Default)]
pub struct BitWriter<W>
where
    W: Write {
    /// * Currently ends at which bit in the last byte
    pub endbit: i32,

    /// * How many bits did we wrote in total
    pub total_bits: usize,

    /// * The sink
    pub writer: W,

    /// * The cache that holds data to be flushed
    pub cache: CursorVecU8,
}

impl<W> BitWriter<W>
where
    W: Write {
    const CACHE_SIZE: usize = 1024;

    /// * Create a `CursorVecU8` to write
    pub fn new(writer: W) -> Self {
        Self {
            endbit: 0,
            total_bits: 0,
            writer,
            cache: CursorVecU8::default(),
        }
    }

    /// * Get the last byte for modifying it
    pub fn last_byte(&mut self) -> &mut u8 {
        if self.cache.is_empty() {
            self.cache.write_all(&[0u8]).unwrap();
        }
        let v = self.cache.get_mut();
        let len = v.len();
        &mut v[len - 1]
    }

    /// * Write data by bytes one by one
    fn write_byte(&mut self, byte: u8) -> Result<(), AudioWriteError> {
        self.cache.write_all(&[byte])?;
        if self.cache.len() >= Self::CACHE_SIZE {
            self.flush()?;
        }
        Ok(())
    }

    /// * Write data in bits, max is 32 bit.
    pub fn write(&mut self, mut value: u32, mut bits: i32) -> Result<(), AudioWriteError> {
        if !(0..=32).contains(&bits) {
            return Err(AudioWriteError::InvalidArguments(format!("Invalid bits {bits}")));
        }
        value &= MASK[bits as usize];
        let origbits = bits;
        bits += self.endbit;

        *self.last_byte() |= (value << self.endbit) as u8;

        if bits >= 8 {
            self.write_byte((value >> (8 - self.endbit)) as u8)?;
            if bits >= 16 {
                self.write_byte((value >> (16 - self.endbit)) as u8)?;
                if bits >= 24 {
                    self.write_byte((value >> (24 - self.endbit)) as u8)?;
                    if bits >= 32 {
                        if self.endbit != 0 {
                            self.write_byte((value >> (32 - self.endbit)) as u8)?;
                        } else {
                            self.write_byte(0)?;
                        }
                    }
                }
            }
        }

        self.endbit = bits & 7;
        self.total_bits += origbits as usize;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), AudioWriteError> {
        if self.cache.is_empty() {
            Ok(())
        } else if self.endbit == 0 {
            self.writer.write_all(&self.cache[..])?;
            self.cache.clear();
            Ok(())
        } else {
            let len = self.cache.len();
            let last_byte = self.cache[len - 1];
            self.writer.write_all(&self.cache[..(len - 1)])?;
            self.cache.clear();
            self.cache.write_all(&[last_byte])?;
            Ok(())
        }
    }

    pub fn force_flush(&mut self) -> Result<(), AudioWriteError> {
        self.writer.write_all(&self.cache[..])?;
        self.cache.clear();
        self.endbit = 0;
        Ok(())
    }
}

/// * The specialized `BitWriter` that uses `CursorVecU8>` as its sink.
pub type BitWriterCursor = BitWriter<CursorVecU8>;

/// * The specialized `BitWriter` that uses `Box<dyn Writer>` as its sink.
pub type BitWriterObj = BitWriter<Box<dyn Writer>>;

impl BitWriterCursor {
    /// * Get the inner byte array and consumes the writer.
    pub fn into_bytes(mut self) -> Vec<u8> {
        // Make sure the last byte was written
        self.force_flush().unwrap();
        self.writer.into_inner()
    }
}

/// * Read bits of data using the environment `bitreader` variable, an instance of `BitReader`
macro_rules! read_bits {
    ($bitreader:ident, $bits:expr) => {
        if DEBUG_ON_READ_BITS {
            $bitreader.read($bits).unwrap()
        } else {
            $bitreader.read($bits)?
        }
    };
}

/// * Write bits of data using the environment `bitwriter` variable, an instance of `BitWriter<W>`
macro_rules! write_bits {
    ($bitwriter:ident, $data:expr, $bits:expr) => {
        if DEBUG_ON_WRITE_BITS {
            $bitwriter.write($data as u32, $bits).unwrap()
        } else {
            $bitwriter.write($data as u32, $bits)?
        }
    };
}

/// * Read a byte array `slice` using the `BitReader`
macro_rules! read_slice {
    ($bitreader:ident, $length:expr) => {
        {
            let mut ret = Vec::<u8>::with_capacity($length);
            for _ in 0..$length {
                ret.push(read_bits!($bitreader, 8) as u8);
            }
            ret
        }
    };
}

/// * Read a sized string using the `BitReader`
macro_rules! read_string {
    ($bitreader:ident, $length:expr) => {
        {
            let s = read_slice!($bitreader, $length);
            match std::str::from_utf8(&s) {
                Ok(s) => Ok(s.to_string()),
                Err(_) => Err(AudioError::InvalidData(format!("Parse UTF-8 failed: {}", String::from_utf8_lossy(&s)))),
            }
        }
    };
}

/// * Write a slice to the `BitWriter`
macro_rules! write_slice {
    ($bitwriter:ident, $data:expr) => {
        for &data in $data.iter() {
            write_bits!($bitwriter, data, mem::size_of_val(&data) as i32 * 8);
        }
    };
}

/// * Write a sized string to the `BitWriter`
macro_rules! write_string {
    ($bitwriter:ident, $string:expr) => {
        write_slice!($bitwriter, $string.as_bytes());
    };
}

/// * Any Vorbis objects that can pack into bitstreams should implement this `VorbisPackableObject`
pub trait VorbisPackableObject {
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError> where W: Write;

    fn to_packed(&self) -> Result<BitwiseData, AudioWriteError> {
        let mut bitwriter = BitWriterCursor::default();
        let bits = self.pack(&mut bitwriter)?;
        Ok(BitwiseData::new(&bitwriter.into_bytes(), bits))
    }
}

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
    pub fn load(bitreader: &mut BitReader) -> Result<Self, AudioReadError> {
        let mut ret = Self::default();
        ret.parse_book(bitreader)?;
        Ok(ret)
    }

    fn parse_book(&mut self, bitreader: &mut BitReader) -> Result<(), AudioReadError> {
        /* make sure alignment is correct */
        if read_bits!(bitreader, 24) != 0x564342 {
            return Err(AudioReadError::FormatError("Check the `BCV` flag failed.".to_string()));
        }

        /* first the basic parameters */
        self.dim = read_bits!(bitreader, 16);
        self.entries = read_bits!(bitreader, 24);
        if ilog!(self.dim) + ilog!(self.entries) > 24 {
            return Err(AudioReadError::FormatError(format!("{} + {} > 24", ilog!(self.dim), ilog!(self.entries))));
        }

        /* codeword ordering.... length ordered or unordered? */
        match read_bits!(bitreader, 1) {
            0 => {
                /* allocated but unused entries? */
                let unused = read_bits!(bitreader, 1) != 0;

                /* unordered */
                self.lengthlist.resize(self.entries as usize, 0);

                /* allocated but unused entries? */
                if unused {
                    /* yes, unused entries */
                    for i in 0..self.entries as usize {
                        if read_bits!(bitreader, 1) != 0 {
                            let num = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                            self.lengthlist[i] = num;
                        } else {
                            self.lengthlist[i] = 0;
                        }
                    }
                } else { /* all entries used; no tagging */
                    for i in 0..self.entries as usize {
                        let num = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                        self.lengthlist[i] = num;
                    }
                }
            }
            1 => { /* ordered */
                let mut length = read_bits!(bitreader, 5).wrapping_add(1) as i8;
                self.lengthlist.resize(self.entries as usize, 0);
                let mut i = 0;
                while i < self.entries {
                    let num = read_bits!(bitreader, ilog!(self.entries - i));
                    if length > 32 || num > self.entries - i || (num > 0 && (num - 1) >> (length - 1) > 1) {
                        return Err(AudioReadError::FormatError(format!("length({length}) > 32 || num({num}) > entries({}) - i({i}) || (num({num}) > 0 && (num({num}) - 1) >> (length({length}) - 1) > 1)", self.entries)));
                    }
                    for _ in 0..num {
                        self.lengthlist[i as usize] = length;
                        i += 1;
                    }
                    length += 1;
                }
            }
            o => return Err(AudioReadError::FormatError(format!("Unexpected codeword ordering {o}"))),
        }

        /* Do we have a mapping to unpack? */
        self.maptype = read_bits!(bitreader, 4);
        match self.maptype {
            0 => (),
            1 | 2 => {
                /* implicitly populated value mapping */
                /* explicitly populated value mapping */
                self.q_min = read_bits!(bitreader, 32);
                self.q_delta = read_bits!(bitreader, 32);
                self.q_quant = read_bits!(bitreader, 4).wrapping_add(1);
                self.q_sequencep = read_bits!(bitreader, 1);

                let quantvals = match self.maptype {
                    1 => if self.dim == 0 {0} else {self.book_maptype1_quantvals() as usize},
                    2 => self.entries as usize * self.dim as usize,
                    _ => unreachable!(),
                };

                /* quantized values */
                self.quantlist.resize(quantvals, 0);
                for i in 0..quantvals {
                    self.quantlist[i] = read_bits!(bitreader, self.q_quant);
                }
            }
            o => return Err(AudioReadError::FormatError(format!("Unexpected maptype {o}"))),
        }
        Ok(())
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
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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
                    return Err(AudioWriteError::MissingData("Missing quantlist data".to_string()));
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
            o => return Err(AudioWriteError::InvalidData(format!("Unexpected maptype {o}"))),
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
    pub fn unpack(&self) -> Result<CodeBooks, AudioReadError> {
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
    pub fn load(bitreader: &mut BitReader) -> Result<Self, AudioReadError> {
        let num_books = (bitreader.read(8)? + 1) as usize;
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
            total_bits: bitreader.total_bits,
        })
    }

    /// * Unpack from a slice
    pub fn load_from_slice(data: &[u8]) -> Result<Self, AudioReadError> {
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
    pub fn to_packed_codebooks(&self) -> Result<CodeBooksPacked, AudioWriteError> {
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
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        write_bits!(bitwriter, self.books.len().wrapping_sub(1), 8);
        for book in self.books.iter() {
            book.pack(bitwriter)?;
        }
        Ok(bitwriter.total_bits - begin_bits)
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
    pub framing_flag: bool,
}

impl VorbisIdentificationHeader {
    /// * Unpack from a bitstream
    pub fn load(bitreader: &mut BitReader) -> Result<Self, AudioReadError> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x01vorbis" {
            Err(AudioReadError::InvalidData(format!("Not a Vorbis identification header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
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
            let framing_flag = read_bits!(bitreader, 1) & 1 == 1;
            if sample_rate < 1
            || channels < 1
            || block_size[0] < 64
            || block_size[1] < block_size[0]
            || block_size[1] > 8192
            || !framing_flag {
                Err(AudioReadError::InvalidData("Bad Vorbis identification header.".to_string()))
            } else {
                Ok(Self {
                    version,
                    channels,
                    sample_rate,
                    bitrate_upper,
                    bitrate_nominal,
                    bitrate_lower,
                    block_size,
                    framing_flag,
                })
            }
        }
    }

    /// * Unpack from a slice
    pub fn load_from_slice(data: &[u8]) -> Result<Self, AudioReadError> {
        let mut bitreader = BitReader::new(data);
        Self::load(&mut bitreader)
    }
}

impl VorbisPackableObject for VorbisIdentificationHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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
        write_bits!(bitwriter, bs_1.wrapping_sub(1), 4);
        write_bits!(bitwriter, bs_2.wrapping_sub(1), 4);
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
    pub fn load(bitreader: &mut BitReader) -> Result<Self, AudioReadError> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x03vorbis" {
            Err(AudioReadError::InvalidData(format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let vendor_len = read_bits!(bitreader, 32);
            if vendor_len < 0 {
                return Err(AudioReadError::InvalidData(format!("Bad vendor string length {vendor_len}")));
            }
            let vendor = read_string!(bitreader, vendor_len as usize)?;
            let num_comments = read_bits!(bitreader, 32);
            if num_comments < 0 {
                return Err(AudioReadError::InvalidData(format!("Bad number of comments {num_comments}")));
            }
            let mut comments = Vec::<String>::with_capacity(num_comments as usize);
            for _ in 0..num_comments {
                let comment_len = read_bits!(bitreader, 32);
                if comment_len < 0 {
                    return Err(AudioReadError::InvalidData(format!("Bad comment string length {vendor_len}")));
                }
                comments.push(read_string!(bitreader, comment_len as usize)?);
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
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        write_slice!(bitwriter, b"\x03vorbis");
        write_bits!(bitwriter, self.vendor.len(), 32);
        write_string!(bitwriter, self.vendor);
        for comment in self.comments.iter() {
            write_bits!(bitwriter, comment.len(), 32);
            write_string!(bitwriter, comment);
        }
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, AudioReadError> {
        let floor_type = bitreader.read(16)? as u16;
        match floor_type {
            0 => Ok(VorbisFloor0::load(bitreader, vorbis_info)?),
            1 => Ok(VorbisFloor1::load(bitreader, vorbis_info)?),
            o => Err(AudioReadError::InvalidData(format!("Invalid floor type {o}"))),
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
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, AudioReadError> {
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
            return Err(AudioReadError::InvalidData(format!("Invalid floor 0 data: \norder = {}\nrate = {}\nbarkmap = {}\nnum_books = {num_books}",
                ret.order,
                ret.rate,
                ret.barkmap
            )));
        }

        for _ in 0..num_books {
            let book = read_bits!(bitreader, 8);
            if book < 0 || book as usize >= static_codebooks.len() {
                return Err(AudioReadError::InvalidData(format!("Invalid book number: {book}")));
            }
            if static_codebooks[book as usize].maptype == 0 {
                return Err(AudioReadError::InvalidData("Invalid book maptype: 0".to_string()));
            }
            if static_codebooks[book as usize].dim < 1 {
                return Err(AudioReadError::InvalidData("Invalid book dimension: 0".to_string()));
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
    pub class_subbook: CopiableBuffer<[i32; 8], 16>,

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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<VorbisFloor, AudioReadError> {
        let static_codebooks = &vorbis_info.static_codebooks;
        let mut ret = Self::default();
        let mut maxclass = -1i32;

        ret.partitions = read_bits!(bitreader, 5);
        ret.partitions_class.resize(ret.partitions as usize, 0);
        for i in 0..ret.partitions as usize {
            let partitions_class = read_bits!(bitreader, 4);
            maxclass = max(maxclass, partitions_class);
            ret.partitions_class[i] = partitions_class;
        }
        let maxclass = maxclass as usize + 1;
        ret.class_dim.resize(maxclass, 0);
        ret.class_subs.resize(maxclass, 0);
        ret.class_book.resize(maxclass, 0);
        ret.class_subbook.resize(maxclass, [0; 8]);

        for i in 0..maxclass {
            ret.class_dim[i] = read_bits!(bitreader, 3).wrapping_add(1);
            ret.class_subs[i] = read_bits!(bitreader, 2);
            if ret.class_subs[i] != 0 {
                ret.class_book[i] = read_bits!(bitreader, 8);
            }
            if ret.class_book[i] as usize >= static_codebooks.len() {
                return Err(AudioReadError::InvalidData(format!("Invalid class book index {}, max books is {}", ret.class_book[i], static_codebooks.len())));
            }
            for k in 0..(1 << ret.class_subs[i]) {
                let subbook_index = read_bits!(bitreader, 8).wrapping_sub(1);
                if subbook_index < -1 || subbook_index >= static_codebooks.len() as i32 {
                    return Err(AudioReadError::InvalidData(format!("Invalid class subbook index {subbook_index}, max books is {}", static_codebooks.len())));
                }
                ret.class_subbook[i][k] = subbook_index;
            }
        }

        ret.mult = read_bits!(bitreader, 2).wrapping_add(1);
        let rangebits = read_bits!(bitreader, 4);

        let mut k = 0usize;
        let mut count = 0usize;
        for i in 0..ret.partitions as usize {
            count += ret.class_dim[ret.partitions_class[i] as usize] as usize;
            if count > 63 {
                return Err(AudioReadError::InvalidData(format!("Invalid class dim sum {count}, max is 63")));
            }
            ret.postlist.resize(count + 2, 0);
            while k < count {
                let t = read_bits!(bitreader, rangebits);
                if t < 0 || t >= (1 << rangebits) {
                    return Err(AudioReadError::InvalidData(format!("Invalid value for postlist {t}")));
                }
                ret.postlist[k + 2] = t;
                k += 1;
            }
        }
        ret.postlist[0] = 0;
        ret.postlist[1] = 1 << rangebits;
        ret.postlist[..(count + 2)].sort();
        for i in 1..(count + 2) {
            if ret.postlist[i - 1] == ret.postlist[i] {
                return Err(AudioReadError::InvalidData(format!("Bad postlist: [{}]", format_array!(ret.postlist, ", ", "{}"))));
            }
        }

        Ok(VorbisFloor::Floor1(ret))
    }
}

impl VorbisPackableObject for VorbisFloor1 {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;
        let maxposit = self.postlist[1];
        let rangebits = ilog!(maxposit - 1);
        let mut maxclass = -1i32;
        write_bits!(bitwriter, 1, 16);
        write_bits!(bitwriter, self.partitions, 5);
        for i in 0..self.partitions as usize {
            let partitions_class = self.partitions_class[i];
            maxclass = max(maxclass, partitions_class);
            write_bits!(bitwriter, partitions_class, 4);
        }
        let maxclass = maxclass as usize + 1;
        for i in 0..maxclass {
            write_bits!(bitwriter, self.class_dim[i].wrapping_sub(1), 3);
            write_bits!(bitwriter, self.class_subs[i], 2);
            if self.class_subs[i] != 0 {
                write_bits!(bitwriter, self.class_book[i], 8);
            }
            for k in 0..(1 << self.class_subs[i]) as usize {
                write_bits!(bitwriter, self.class_subbook[i][k].wrapping_add(1), 8);
            }
        }
        write_bits!(bitwriter, self.mult.wrapping_sub(1), 2);
        write_bits!(bitwriter, rangebits, 4);
        let mut k = 0usize;
        let mut count = 0usize;
        for i in 0..self.partitions as usize {
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<Self, AudioReadError> {
        let static_codebooks = &vorbis_info.static_codebooks;
        let residue_type = read_bits!(bitreader, 16);

        if !(0..3).contains(&residue_type) {
            return Err(AudioReadError::InvalidData(format!("Invalid residue type {residue_type}")))
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
            return Err(AudioReadError::InvalidData(format!("Invalid groupbook index {}", ret.groupbook)));
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
                return Err(AudioReadError::InvalidData(format!("Invalid book index {book}")));
            }
            ret.booklist[i] = book;
            let book_maptype = static_codebooks[book as usize].maptype;
            if book_maptype == 0 {
                return Err(AudioReadError::InvalidData(format!("Invalid book maptype {book_maptype}")));
            }
        }

        let groupbook = &static_codebooks[ret.groupbook as usize];
        let entries = groupbook.entries;
        let mut dim = groupbook.dim;
        let mut partvals = 1i32;
        if dim < 1 {
            return Err(AudioReadError::InvalidData(format!("Invalid groupbook dimension {dim}")));
        }
        while dim > 0 {
            partvals *= ret.partitions;
            if partvals > entries {
                return Err(AudioReadError::InvalidData(format!("Invalid partvals {partvals}")));
            }
            dim -= 1;
        }
        ret.partvals = partvals;
        Ok(ret)
    }
}

impl VorbisPackableObject for VorbisResidue {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader, ident_header: &VorbisIdentificationHeader) -> Result<Self, AudioReadError> {
        let mapping_type = read_bits!(bitreader, 16);

        if mapping_type != 0 {
            return Err(AudioReadError::InvalidData(format!("Invalid mapping type {mapping_type}")))
        }

        let channels = ident_header.channels as i32;
        let floors = vorbis_info.floors.len() as i32;
        let residues = vorbis_info.residues.len() as i32;
        let submaps = if read_bits!(bitreader, 1) != 0 {
            let submaps = read_bits!(bitreader, 4).wrapping_add(1);
            if submaps == 0 {
                return Err(AudioReadError::InvalidData("No submaps.".to_string()));
            }
            submaps
        } else {
            1
        };
        let coupling_steps = if read_bits!(bitreader, 1) != 0 {
            let coupling_steps = read_bits!(bitreader, 8).wrapping_add(1);
            if coupling_steps == 0 {
                return Err(AudioReadError::InvalidData("No coupling steps.".to_string()));
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
                return Err(AudioReadError::InvalidData(format!("Bad values for test_m = {test_m}, test_a = {test_a}, channels = {channels}")));
            }
        }

        let reserved = read_bits!(bitreader, 2);
        if reserved != 0 {
            return Err(AudioReadError::InvalidData(format!("Reserved value is {reserved}")));
        }

        if submaps > 1 {
            ret.chmuxlist.resize(channels, 0);
            for i in 0..channels {
                let chmux = read_bits!(bitreader, 4);
                if chmux >= submaps as i32 {
                    return Err(AudioReadError::InvalidData(format!("Chmux {chmux} >= submaps {submaps}")));
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
                return Err(AudioReadError::InvalidData(format!("floorsubmap {floorsubmap} >= floors {floors}")));
            }
            ret.floorsubmap[i] = floorsubmap;
            let residuesubmap = read_bits!(bitreader, 8);
            if residuesubmap >= residues {
                return Err(AudioReadError::InvalidData(format!("floorsubmap {floorsubmap} >= floors {floors}")));
            }
            ret.residuesubmap[i] = residuesubmap;
        }
        Ok(ret)
    }
}

impl VorbisPackableObject for VorbisMapping {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> Result<Self, AudioReadError> {
        let ret = Self {
            block_flag: read_bits!(bitreader, 1) != 0,
            window_type: read_bits!(bitreader, 16),
            transform_type: read_bits!(bitreader, 16),
            mapping: read_bits!(bitreader, 8),
        };

        if ret.window_type != 0 {
            Err(AudioReadError::InvalidData(format!("Bad window type: {}", ret.window_type)))
        } else if ret.transform_type != 0 {
            Err(AudioReadError::InvalidData(format!("Bad transfrom type: {}", ret.transform_type)))
        } else if ret.mapping as usize >= vorbis_info.maps.len() {
            Err(AudioReadError::InvalidData(format!("Mapping exceeded boundary: {} >= {}", ret.mapping, vorbis_info.maps.len())))
        } else {
            Ok(ret)
        }
    }
}

impl VorbisPackableObject for VorbisMode {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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

/// * The `VorbisSetupHeader` is the Vorbis setup header, the second header
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
    pub fn load(bitreader: &mut BitReader, ident_header: &VorbisIdentificationHeader) -> Result<Self, AudioReadError> {
        let ident = read_slice!(bitreader, 7);
        if ident != b"\x05vorbis" {
            Err(AudioReadError::InvalidData(format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
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
                    return Err(AudioReadError::InvalidData(format!("Invalid time type {time_type}")));
                }
            }

            // floor backend settings
            let floors = read_bits!(bitreader, 6).wrapping_add(1);
            if floors == 0 {
                return Err(AudioReadError::InvalidData("No floor backend settings.".to_string()));
            }
            for _ in 0..floors {
                ret.floors.push(VorbisFloor::load(bitreader, &ret)?);
            }

            // residue backend settings
            let residues = read_bits!(bitreader, 6).wrapping_add(1);
            if residues == 0 {
                return Err(AudioReadError::InvalidData("No residues backend settings.".to_string()));
            }
            for _ in 0..residues {
                ret.residues.push(VorbisResidue::load(bitreader, &ret)?);
            }

            // map backend settings
            let maps = read_bits!(bitreader, 6).wrapping_add(1);
            if maps == 0 {
                return Err(AudioReadError::InvalidData("No map backend settings.".to_string()));
            }
            for _ in 0..maps {
                ret.maps.push(VorbisMapping::load(bitreader, &ret, ident_header)?);
            }

            // mode settings
            let modes = read_bits!(bitreader, 6).wrapping_add(1);
            if modes == 0 {
                return Err(AudioReadError::InvalidData("No mode settings.".to_string()));
            }
            for _ in 0..modes {
                ret.modes.push(VorbisMode::load(bitreader, &ret)?);
            }

            let eop = read_bits!(bitreader, 1) != 0;
            if !eop {
                return Err(AudioReadError::InvalidData("Missing End Of Packet bit.".to_string()));
            }

            Ok(ret)
        }
    }
}

impl VorbisPackableObject for VorbisSetupHeader {
    /// * Pack to the bitstream
    fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, AudioWriteError>
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

/// * This function extracts data from some Ogg packets, the packets contains the Vorbis headers.
/// * There are 3 kinds of Vorbis headers, they are the identification header, the metadata header, and the setup header.
#[allow(clippy::type_complexity)]
pub fn get_vorbis_headers_from_ogg_packet_bytes(data: &[u8], stream_id: &mut u32) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), AudioError> {
    use crate::ogg::OggPacket;
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
                o => return Err(AudioError::Unparseable(format!("Invalid Vorbis header type {o}"))),
            }
        }
    }

    *stream_id = ogg_packets[0].stream_id;
    Ok((ident_header, metadata_header, setup_header))
}

/// * This function extracts data from Ogg packets, the packets contains the Vorbis header.
/// * The packets were all decoded.
pub fn parse_vorbis_headers(data: &[u8], stream_id: &mut u32) -> Result<(VorbisIdentificationHeader, VorbisCommentHeader, VorbisSetupHeader), AudioReadError> {
    let (b1, b2, b3) = get_vorbis_headers_from_ogg_packet_bytes(data, stream_id)?;
    debugln!("b1 = [{}]", format_array!(b1, " ", "{:02x}"));
    debugln!("b2 = [{}]", format_array!(b2, " ", "{:02x}"));
    debugln!("b3 = [{}]", format_array!(b3, " ", "{:02x}"));
    let mut br1 = BitReader::new(&b1);
    let mut br2 = BitReader::new(&b2);
    let mut br3 = BitReader::new(&b3);
    let h1 = VorbisIdentificationHeader::load(&mut br1).unwrap();
    let h2 = VorbisCommentHeader::load(&mut br2).unwrap();
    let h3 = VorbisSetupHeader::load(&mut br3, &h1).unwrap();
    Ok((h1, h2, h3))
}

/// * This function removes the codebooks from the Vorbis setup header. The setup header was extracted from the Ogg stream.
/// * Since Vorbis stores data in bitwise form, all of the data are not aligned in bytes, we have to parse it bit by bit.
/// * After parsing the codebooks, we can sum up the total bits of the codebooks, and then we can replace it with an empty codebook.
/// * At last, use our `BitwiseData` to concatenate these bit-strings without any gaps.
pub fn remove_codebook_from_setup_header(setup_header: &[u8]) -> Result<Vec<u8>, AudioError> {
    // Try to verify if this is the right way to read the codebook
    assert_eq!(&setup_header[0..7], b"\x05vorbis", "Checking the vorbis header that is a `setup_header` or not");

    // Let's find the book, and kill it.
    let codebooks = CodeBooks::load_from_slice(&setup_header[7..]).unwrap();
    let bytes_before_codebook = BitwiseData::from_bytes(&setup_header[0..7]);
    let (_codebook_bits, bits_after_codebook) = BitwiseData::new(&setup_header[7..], (setup_header.len() - 7) * 8).split(codebooks.total_bits);

    // Let's generate the empty codebook.
    let _empty_codebooks = CodeBooks::default().to_packed_codebooks().unwrap().books;

    let mut setup_header = BitwiseData::default();
    setup_header.concat(&bytes_before_codebook);
    setup_header.concat(&_empty_codebooks);
    setup_header.concat(&bits_after_codebook);

    Ok(setup_header.into_bytes())
}

/// * This function removes all codebooks from the Vorbis Setup Header.
/// * To think normally, when the codebooks in the Vorbis audio data were removed, the Vorbis audio was unable to decode.
/// * This function exists because the author of `Vorbis ACM` registered `FORMAT_TAG_OGG_VORBIS3` and `FORMAT_TAG_OGG_VORBIS3P`, and its comment says "Have no codebook header".
/// * I thought if I wanted to encode/decode this kind of Vorbis audio, I might have to remove the codebooks when encoding.
/// * After days of re-inventing the wheel of Vorbis bitwise read/writer and codebook parser and serializer, and being able to remove the codebook, then, BAM, I knew I was pranked by the Japanese author.
/// * I have his decoder source code, when I read it carefully, I found out that he just stripped the whole Vorbis header for `FORMAT_TAG_OGG_VORBIS3` and `FORMAT_TAG_OGG_VORBIS3P`.
/// * And when decoding, he creates a temporary encoder with parameters referenced from the `fmt ` chunk, uses that encoder to create the Vorbis header to feed the decoder, and then can decode the Vorbis audio.
/// * It has nothing to do with the codebook. I was pranked.
/// * Thanks, the source code from 2001, and the author from Japan.
pub fn _remove_codebook_from_ogg_stream(data: &[u8]) -> Result<Vec<u8>, AudioError> {
    use crate::ogg::{OggPacket, OggPacketType};
    let mut stream_id = 0u32;
    let (identification_header, comment_header, setup_header) = get_vorbis_headers_from_ogg_packet_bytes(data, &mut stream_id)?;

    // Our target is to kill the codebooks from the `setup_header`
    // If this packet doesn't have any `setup_header`
    // We return.
    if setup_header.is_empty() {
        return Err(AudioError::NoSuchData("There's no setup header in the given Ogg packets.".to_string()));
    }

    let setup_header = remove_codebook_from_setup_header(&setup_header)?;

    let mut identification_header_packet = OggPacket::new(stream_id, OggPacketType::BeginOfStream, 0);
    let mut comment_header_packet = OggPacket::new(stream_id, OggPacketType::Continuation, 1);
    let mut setup_header_packet = OggPacket::new(stream_id, OggPacketType::Continuation, 2);
    identification_header_packet.write(&identification_header);
    comment_header_packet.write(&comment_header);
    setup_header_packet.write(&setup_header);

    Ok([identification_header_packet.into_bytes(), comment_header_packet.into_bytes(), setup_header_packet.into_bytes()].into_iter().flatten().collect())
}


#[test]
fn test_parse_vorbis_headers() {
    let test_header = [
        0x4F, 0x67, 0x67, 0x53, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xE8, 0xFF,
        0x0D, 0x9F, 0x00, 0x00, 0x00, 0x00, 0x5D, 0x70, 0x38, 0xED, 0x01, 0x1E, 0x01, 0x76, 0x6F, 0x72,
        0x62, 0x69, 0x73, 0x00, 0x00, 0x00, 0x00, 0x02, 0x44, 0xAC, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
        0x00, 0xE2, 0x04, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xB8, 0x01, 0x4F, 0x67, 0x67, 0x53, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xE8, 0xFF, 0x0D, 0x9F, 0x01, 0x00, 0x00, 0x00,
        0xA4, 0xC9, 0xCB, 0x01, 0x11, 0x52, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x53, 0x03, 0x76, 0x6F, 0x72, 0x62, 0x69, 0x73, 0x2F, 0x00, 0x00,
        0x00, 0x41, 0x4F, 0x3B, 0x20, 0x61, 0x6F, 0x54, 0x75, 0x56, 0x20, 0x5B, 0x32, 0x30, 0x31, 0x31,
        0x30, 0x34, 0x32, 0x34, 0x5D, 0x20, 0x28, 0x62, 0x61, 0x73, 0x65, 0x64, 0x20, 0x6F, 0x6E, 0x20,
        0x6C, 0x69, 0x62, 0x76, 0x6F, 0x72, 0x62, 0x69, 0x73, 0x20, 0x31, 0x2E, 0x33, 0x2E, 0x37, 0x29,
        0x01, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x45, 0x4E, 0x43, 0x4F, 0x44, 0x45, 0x52, 0x3D,
        0x72, 0x75, 0x73, 0x74, 0x77, 0x61, 0x76, 0x01, 0x05, 0x76, 0x6F, 0x72, 0x62, 0x69, 0x73, 0x2B,
        0x42, 0x43, 0x56, 0x01, 0x00, 0x08, 0x00, 0x00, 0x00, 0x31, 0x4C, 0x20, 0xC5, 0x80, 0xD0, 0x90,
        0x55, 0x00, 0x00, 0x10, 0x00, 0x00, 0x60, 0x24, 0x29, 0x0E, 0x93, 0x66, 0x49, 0x29, 0xA5, 0x94,
        0xA1, 0x28, 0x79, 0x98, 0x94, 0x48, 0x49, 0x29, 0xA5, 0x94, 0xC5, 0x30, 0x89, 0x98, 0x94, 0x89,
        0xC5, 0x18, 0x63, 0x8C, 0x31, 0xC6, 0x18, 0x63, 0x8C, 0x31, 0xC6, 0x18, 0x63, 0x8C, 0x20, 0x34,
        0x64, 0x15, 0x00, 0x00, 0x04, 0x00, 0x80, 0x28, 0x09, 0x8E, 0xA3, 0xE6, 0x49, 0x6A, 0xCE, 0x39,
        0x67, 0x18, 0x27, 0x8E, 0x72, 0xA0, 0x39, 0x69, 0x4E, 0x38, 0xA7, 0x20, 0x07, 0x8A, 0x51, 0xE0,
        0x39, 0x09, 0xC2, 0xF5, 0x26, 0x63, 0x6E, 0xA6, 0xB4, 0xA6, 0x6B, 0x6E, 0xCE, 0x29, 0x25, 0x08,
        0x0D, 0x59, 0x05, 0x00, 0x00, 0x02, 0x00, 0x40, 0x48, 0x21, 0x85, 0x14, 0x52, 0x48, 0x21, 0x85,
        0x14, 0x62, 0x88, 0x21, 0x86, 0x18, 0x62, 0x88, 0x21, 0x87, 0x1C, 0x72, 0xC8, 0x21, 0xA7, 0x9C,
        0x72, 0x0A, 0x2A, 0xA8, 0xA0, 0x82, 0x0A, 0x32, 0xC8, 0x20, 0x83, 0x4C, 0x32, 0xE9, 0xA4, 0x93,
        0x4E, 0x3A, 0xE9, 0xA8, 0xA3, 0x8E, 0x3A, 0xEA, 0x28, 0xB4, 0xD0, 0x42, 0x0B, 0x2D, 0xB4, 0xD2,
        0x4A, 0x4C, 0x31, 0xD5, 0x56, 0x63, 0xAE, 0xBD, 0x06, 0x5D, 0x7C, 0x73, 0xCE, 0x39, 0xE7, 0x9C,
        0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x73, 0xCE, 0x09, 0x42, 0x43, 0x56, 0x01, 0x00, 0x20, 0x00, 0x00,
        0x04, 0x42, 0x06, 0x19, 0x64, 0x10, 0x42, 0x08, 0x21, 0x85, 0x14, 0x52, 0x88, 0x29, 0xA6, 0x98,
        0x72, 0x0A, 0x32, 0xC8, 0x80, 0xD0, 0x90, 0x55, 0x00, 0x00, 0x20, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x47, 0x91, 0x14, 0x49, 0xB1, 0x14, 0xCB, 0xB1, 0x1C, 0xCD, 0xD1, 0x24, 0x4F, 0xF2, 0x2C,
        0x51, 0x13, 0x35, 0xD1, 0x33, 0x45, 0x53, 0x54, 0x4D, 0x55, 0x55, 0x55, 0x55, 0x75, 0x5D, 0x57,
        0x76, 0x65, 0xD7, 0x76, 0x75, 0xD7, 0x76, 0x7D, 0x59, 0x98, 0x85, 0x5B, 0xB8, 0x7D, 0x59, 0xB8,
        0x85, 0x5B, 0xD8, 0x85, 0x5D, 0xF7, 0x85, 0x61, 0x18, 0x86, 0x61, 0x18, 0x86, 0x61, 0x18, 0x86,
        0x61, 0xF8, 0x7D, 0xDF, 0xF7, 0x7D, 0xDF, 0xF7, 0x7D, 0x20, 0x34, 0x64, 0x15, 0x00, 0x20, 0x01,
        0x00, 0xA0, 0x23, 0x39, 0x96, 0xE3, 0x29, 0xA2, 0x22, 0x1A, 0xA2, 0xE2, 0x39, 0xA2, 0x03, 0x84,
        0x86, 0xAC, 0x02, 0x00, 0x64, 0x00, 0x00, 0x04, 0x00, 0x20, 0x09, 0x92, 0x22, 0x29, 0x92, 0xA3,
        0x49, 0xA6, 0x66, 0x6A, 0xAE, 0x69, 0x9B, 0xB6, 0x68, 0xAB, 0xB6, 0x6D, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x0C, 0x84, 0x86, 0xAC, 0x02, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xA0, 0x69, 0x9A, 0xA6, 0x69, 0x9A, 0xA6, 0x69, 0x9A, 0xA6, 0x69, 0x9A, 0xA6, 0x69, 0x9A, 0xA6,
        0x69, 0x9A, 0xA6, 0x69, 0x9A, 0x66, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59,
        0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96,
        0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x40, 0x68, 0xC8, 0x2A, 0x00,
        0x40, 0x02, 0x00, 0x40, 0xC7, 0x71, 0x1C, 0xC7, 0x71, 0x24, 0x45, 0x52, 0x24, 0xC7, 0x72, 0x2C,
        0x07, 0x08, 0x0D, 0x59, 0x05, 0x00, 0xC8, 0x00, 0x00, 0x08, 0x00, 0x40, 0x52, 0x2C, 0xC5, 0x72,
        0x34, 0x47, 0x73, 0x34, 0xC7, 0x73, 0x3C, 0xC7, 0x73, 0x3C, 0x47, 0x74, 0x44, 0xC9, 0x94, 0x4C,
        0xCD, 0xF4, 0x4C, 0x0F, 0x08, 0x0D, 0x59, 0x05, 0x00, 0x00, 0x02, 0x00, 0x08, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x40, 0x31, 0x1C, 0xC5, 0x71, 0x1C, 0xC9, 0xD1, 0x24, 0x4F, 0x52, 0x2D, 0xD3, 0x72,
        0x35, 0x57, 0x73, 0x3D, 0xD7, 0x73, 0x4D, 0xD7, 0x75, 0x5D, 0x57, 0x55, 0x55, 0x55, 0x55, 0x55,
        0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
        0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x81, 0xD0, 0x90,
        0x55, 0x00, 0x00, 0x04, 0x00, 0x00, 0x21, 0x9D, 0x66, 0x96, 0x6A, 0x80, 0x08, 0x33, 0x90, 0x61,
        0x20, 0x34, 0x64, 0x15, 0x00, 0x80, 0x00, 0x00, 0x00, 0x18, 0xA1, 0x08, 0x43, 0x0C, 0x08, 0x0D,
        0x59, 0x05, 0x00, 0x00, 0x04, 0x00, 0x00, 0x88, 0xA1, 0xE4, 0x20, 0x9A, 0xD0, 0x9A, 0xF3, 0xCD,
        0x39, 0x0E, 0x9A, 0xE5, 0xA0, 0xA9, 0x14, 0x9B, 0xD3, 0xC1, 0x89, 0x54, 0x9B, 0x27, 0xB9, 0xA9,
        0x98, 0x9B, 0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x6C, 0xCE, 0x19, 0xE3, 0x9C, 0x73, 0xCE, 0x29, 0xCA,
        0x99, 0xC5, 0xA0, 0x99, 0xD0, 0x9A, 0x73, 0xCE, 0x49, 0x0C, 0x9A, 0xA5, 0xA0, 0x99, 0xD0, 0x9A,
        0x73, 0xCE, 0x79, 0x12, 0x9B, 0x07, 0xAD, 0xA9, 0xD2, 0x9A, 0x73, 0xCE, 0x19, 0xE7, 0x9C, 0x0E,
        0xC6, 0x19, 0x61, 0x9C, 0x73, 0xCE, 0x69, 0xD2, 0x9A, 0x07, 0xA9, 0xD9, 0x58, 0x9B, 0x73, 0xCE,
        0x59, 0xD0, 0x9A, 0xE6, 0xA8, 0xB9, 0x14, 0x9B, 0x73, 0xCE, 0x89, 0x94, 0x9B, 0x27, 0xB5, 0xB9,
        0x54, 0x9B, 0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x73, 0xCE, 0xA9, 0x5E,
        0x9C, 0xCE, 0xC1, 0x39, 0xE1, 0x9C, 0x73, 0xCE, 0x89, 0xDA, 0x9B, 0x6B, 0xB9, 0x09, 0x5D, 0x9C,
        0x73, 0xCE, 0xF9, 0x64, 0x9C, 0xEE, 0xCD, 0x09, 0xE1, 0x9C, 0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x73,
        0xCE, 0x39, 0xE7, 0x9C, 0x73, 0xCE, 0x09, 0x42, 0x43, 0x56, 0x01, 0x00, 0x40, 0x00, 0x00, 0x04,
        0x61, 0xD8, 0x18, 0xC6, 0x9D, 0x82, 0x20, 0x7D, 0x8E, 0x06, 0x62, 0x14, 0x21, 0xA6, 0x21, 0x93,
        0x1E, 0x74, 0x8F, 0x0E, 0x93, 0xA0, 0x31, 0xC8, 0x29, 0xA4, 0x1E, 0x8D, 0x8E, 0x46, 0x4A, 0xA9,
        0x83, 0x50, 0x52, 0x19, 0x27, 0xA5, 0x74, 0x82, 0xD0, 0x90, 0x55, 0x00, 0x00, 0x20, 0x00, 0x00,
        0x84, 0x10, 0x52, 0x48, 0x21, 0x85, 0x14, 0x52, 0x48, 0x21, 0x85, 0x14, 0x52, 0x48, 0x21, 0x86,
        0x18, 0x62, 0x88, 0x21, 0xA7, 0x9C, 0x72, 0x0A, 0x2A, 0xA8, 0xA4, 0x92, 0x8A, 0x2A, 0xCA, 0x28,
        0xB3, 0xCC, 0x32, 0xCB, 0x2C, 0xB3, 0xCC, 0x32, 0xCB, 0xAC, 0xC3, 0xCE, 0x3A, 0xEB, 0xB0, 0xC3,
        0x10, 0x43, 0x0C, 0x31, 0xB4, 0xD2, 0x4A, 0x2C, 0x35, 0xD5, 0x56, 0x63, 0x8D, 0xB5, 0xE6, 0x9E,
        0x73, 0xAE, 0x39, 0x48, 0x6B, 0xA5, 0xB5, 0xD6, 0x5A, 0x2B, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5,
        0x20, 0x34, 0x64, 0x15, 0x00, 0x00, 0x02, 0x00, 0x40, 0x20, 0x64, 0x90, 0x41, 0x06, 0x19, 0x85,
        0x14, 0x52, 0x48, 0x21, 0x86, 0x98, 0x72, 0xCA, 0x29, 0xA7, 0xA0, 0x82, 0x0A, 0x08, 0x0D, 0x59,
        0x05, 0x00, 0x00, 0x02, 0x00, 0x08, 0x00, 0x00, 0x00, 0xF0, 0x24, 0xCF, 0x11, 0x1D, 0xD1, 0x11,
        0x1D, 0xD1, 0x11, 0x1D, 0xD1, 0x11, 0x1D, 0xD1, 0x11, 0x1D, 0xCF, 0xF1, 0x1C, 0x51, 0x12, 0x25,
        0x51, 0x12, 0x25, 0xD1, 0x32, 0x2D, 0x53, 0x33, 0x3D, 0x55, 0x54, 0x55, 0x57, 0x76, 0x6D, 0x59,
        0x97, 0x75, 0xDB, 0xB7, 0x85, 0x5D, 0xD8, 0x75, 0xDF, 0xD7, 0x7D, 0xDF, 0xD7, 0x8D, 0x5F, 0x17,
        0x86, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96, 0x65, 0x59, 0x96,
        0x65, 0x09, 0x42, 0x43, 0x56, 0x01, 0x00, 0x20, 0x00, 0x00, 0x00, 0x42, 0x08, 0x21, 0x84, 0x14,
        0x52, 0x48, 0x21, 0x85, 0x94, 0x62, 0x8C, 0x31, 0xC7, 0x9C, 0x83, 0x4E, 0x42, 0x09, 0x81, 0xD0,
        0x90, 0x55, 0x00, 0x00, 0x20, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x47, 0x71, 0x14, 0xC7, 0x91,
        0x1C, 0xC9, 0x91, 0x24, 0x4B, 0xB2, 0x24, 0x4D, 0xD2, 0x2C, 0xCD, 0xF2, 0x34, 0x4F, 0xF3, 0x34,
        0xD1, 0x13, 0x45, 0x51, 0x34, 0x4D, 0x53, 0x15, 0x5D, 0xD1, 0x15, 0x75, 0xD3, 0x16, 0x65, 0x53,
        0x36, 0x5D, 0xD3, 0x35, 0x65, 0xD3, 0x55, 0x65, 0xD5, 0x76, 0x65, 0xD9, 0xB6, 0x65, 0x5B, 0xB7,
        0x7D, 0x59, 0xB6, 0x7D, 0xDF, 0xF7, 0x7D, 0xDF, 0xF7, 0x7D, 0xDF, 0xF7, 0x7D, 0xDF, 0xF7, 0x7D,
        0xDF, 0xD7, 0x75, 0x20, 0x34, 0x64, 0x15, 0x00, 0x20, 0x01, 0x00, 0xA0, 0x23, 0x39, 0x92, 0x22,
        0x29, 0x92, 0x22, 0x39, 0x8E, 0xE3, 0x48, 0x92, 0x04, 0x84, 0x86, 0xAC, 0x02, 0x00, 0x64, 0x00,
        0x00, 0x04, 0x00, 0xA0, 0x28, 0x8E, 0xE2, 0x38, 0x8E, 0x23, 0x49, 0x92, 0x24, 0x59, 0x92, 0x26,
        0x79, 0x96, 0x67, 0x89, 0x9A, 0xA9, 0x99, 0x9E, 0xE9, 0xA9, 0xA2, 0x0A, 0x84, 0x86, 0xAC, 0x02,
        0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA0, 0x68, 0x8A, 0xA7, 0x98, 0x8A,
        0xA7, 0x88, 0x8A, 0xE7, 0x88, 0x8E, 0x28, 0x89, 0x96, 0x69, 0x89, 0x9A, 0xAA, 0xB9, 0xA2, 0x6C,
        0xCA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA,
        0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0xAE,
        0xEB, 0xBA, 0xAE, 0xEB, 0xBA, 0x40, 0x68, 0xC8, 0x2A, 0x00, 0x40, 0x02, 0x00, 0x40, 0x47, 0x72,
        0x24, 0x47, 0x72, 0x24, 0x45, 0x52, 0x24, 0x45, 0x72, 0x24, 0x07, 0x08, 0x0D, 0x59, 0x05, 0x00,
        0xC8, 0x00, 0x00, 0x08, 0x00, 0xC0, 0x31, 0x1C, 0x43, 0x52, 0x24, 0xC7, 0xB2, 0x2C, 0x4D, 0xF3,
        0x34, 0x4F, 0xF3, 0x34, 0xD1, 0x13, 0x3D, 0xD1, 0x33, 0x3D, 0x55, 0x74, 0x45, 0x17, 0x08, 0x0D,
        0x59, 0x05, 0x00, 0x00, 0x02, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x90, 0x0C, 0x4B,
        0xB1, 0x1C, 0xCD, 0xD1, 0x24, 0x51, 0x52, 0x2D, 0xD5, 0x52, 0x35, 0xD5, 0x52, 0x2D, 0x55, 0x54,
        0x3D, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
        0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
        0x55, 0x55, 0xD5, 0x34, 0x4D, 0xD3, 0x34, 0x81, 0xD0, 0x90, 0x95, 0x00, 0x00, 0x19, 0x00, 0x00,
        0x84, 0xC5, 0x07, 0xA1, 0x8C, 0x52, 0x12, 0x93, 0xD4, 0x5A, 0xEC, 0xC1, 0x58, 0x8A, 0x31, 0x08,
        0xA5, 0x06, 0xE5, 0x31, 0x85, 0x14, 0x83, 0x96, 0x84, 0xC7, 0x98, 0x42, 0xCA, 0x51, 0x4E, 0xA2,
        0x63, 0x0A, 0x21, 0xE5, 0x30, 0xA7, 0xD2, 0x39, 0x86, 0x8C, 0x91, 0xDA, 0x62, 0x0A, 0x99, 0x32,
        0x42, 0x59, 0xF1, 0x3D, 0x76, 0x8C, 0x21, 0x87, 0x3D, 0x18, 0x9D, 0x42, 0xE8, 0x24, 0x06, 0x42,
        0x43, 0x56, 0x04, 0x00, 0x51, 0x00, 0x00, 0x06, 0x49, 0x22, 0x49, 0x24, 0xC9, 0xF2, 0x3C, 0xA2,
        0x47, 0xF4, 0x2C, 0xCF, 0xE3, 0x89, 0x3C, 0x11, 0x80, 0xE4, 0x79, 0x34, 0x8D, 0xE7, 0x49, 0x9E,
        0x47, 0xF3, 0x78, 0x1E, 0x00, 0x49, 0xF4, 0x78, 0x1E, 0x4D, 0x93, 0x3C, 0x91, 0xE7, 0xD1, 0x34,
        0x01, 0x00, 0x00, 0x01, 0x0E, 0x00, 0x00, 0x01, 0x16, 0x42, 0xA1, 0x21, 0x2B, 0x02, 0x80, 0x38,
        0x01, 0x00, 0x8B, 0x24, 0x79, 0x1E, 0x49, 0xF2, 0x3C, 0x92, 0xE4, 0x79, 0x34, 0x4D, 0x14, 0x21,
        0x8A, 0x96, 0xA6, 0x89, 0x1E, 0xCF, 0x13, 0x45, 0x9E, 0x26, 0x8A, 0x44, 0xD3, 0x34, 0xA1, 0x9A,
        0x96, 0xA6, 0x79, 0x22, 0xCF, 0x13, 0x45, 0x9A, 0x27, 0x8A, 0x4C, 0x51, 0x35, 0x61, 0x9A, 0x9E,
        0xE8, 0x99, 0x26, 0xD3, 0x74, 0x55, 0xA6, 0xA9, 0xAA, 0x5C, 0x59, 0x96, 0x21, 0xBB, 0x9E, 0x27,
        0x9A, 0x26, 0xD3, 0x54, 0x5D, 0xA6, 0xA9, 0xAA, 0x64, 0x57, 0x96, 0x21, 0xCB, 0x00, 0x00, 0x00,
        0x2C, 0x4F, 0x33, 0x4D, 0x9A, 0x66, 0x8A, 0x34, 0xCD, 0x34, 0x89, 0xA2, 0x69, 0xC2, 0x34, 0x2D,
        0xCD, 0x33, 0x4D, 0x9A, 0x26, 0x9A, 0x34, 0xCD, 0x34, 0x89, 0xA2, 0x69, 0xC2, 0x34, 0x3D, 0x51,
        0x54, 0x55, 0xA6, 0xA9, 0xAA, 0x4C, 0x53, 0x55, 0xB9, 0xAE, 0xEB, 0xC2, 0x75, 0x3D, 0xD1, 0x54,
        0x55, 0xA2, 0xA9, 0xAA, 0x4C, 0x53, 0x55, 0xB9, 0xAE, 0xEB, 0xC2, 0x75, 0x01, 0x00, 0x00, 0x48,
        0x9E, 0x66, 0x9A, 0x34, 0xCD, 0x34, 0x69, 0x9A, 0x29, 0x12, 0x45, 0xD3, 0x84, 0x69, 0x5A, 0x9A,
        0x67, 0x9A, 0x34, 0xCD, 0x34, 0x69, 0x9A, 0x68, 0x12, 0x45, 0xD3, 0x84, 0x69, 0x7A, 0xA6, 0xE8,
        0xAA, 0x4C, 0xD3, 0x55, 0x99, 0xA2, 0xAA, 0x52, 0x5D, 0xD7, 0x85, 0xEB, 0x7A, 0xA2, 0xA9, 0xBA,
        0x4C, 0x53, 0x55, 0x89, 0xA6, 0xAA, 0x72, 0x55, 0xD7, 0x85, 0xEB, 0x02, 0x00, 0x00, 0xD0, 0x4C,
        0xD1, 0x75, 0x89, 0xA2, 0xAB, 0x12, 0x45, 0x55, 0x65, 0x9A, 0xAE, 0x0A, 0xD5, 0xD5, 0x44, 0xD3,
        0x75, 0x89, 0xA2, 0xEA, 0x12, 0x45, 0x55, 0x65, 0x9A, 0xAA, 0x0B, 0x55, 0x15, 0x55, 0x53, 0x76,
        0x99, 0xA6, 0xEB, 0x32, 0x4D, 0xD7, 0xA5, 0xAA, 0xAE, 0x0B, 0xD9, 0x15, 0x4D, 0xD5, 0x95, 0x99,
        0xA6, 0xEB, 0x32, 0x4D, 0xD7, 0xA5, 0xBA, 0xAE, 0x0B, 0x57, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x80, 0xA8, 0x9A, 0xB2, 0xCC, 0x34, 0x5D, 0x97, 0x69, 0xBA, 0x2E, 0xD5, 0x75,
        0x5D, 0xB8, 0xAE, 0x68, 0xAA, 0xB2, 0xCC, 0x34, 0x5D, 0x97, 0x69, 0xBA, 0x2E, 0x57, 0x95, 0x5D,
        0xB8, 0xAE, 0x00, 0x00, 0x80, 0x01, 0x07, 0x00, 0x80, 0x00, 0x13, 0xCA, 0x40, 0xA1, 0x21, 0x2B,
        0x01, 0x80, 0x28, 0x00, 0x00, 0x8B, 0xE3, 0x48, 0x92, 0x65, 0x79, 0x1E, 0xC7, 0x91, 0x24, 0x4B,
        0xF3, 0x3C, 0x8E, 0x23, 0x49, 0x9A, 0xE6, 0x79, 0x24, 0xC9, 0xB2, 0x34, 0x4D, 0x14, 0x61, 0x59,
        0x9A, 0x26, 0x8A, 0xD0, 0x34, 0xCF, 0x13, 0x45, 0x68, 0x9A, 0xE7, 0x89, 0x22, 0x00, 0x00, 0x02,
        0x00, 0x00, 0x0A, 0x1C, 0x00, 0x00, 0x02, 0x6C, 0xD0, 0x94, 0x58, 0x1C, 0xA0, 0xD0, 0x90, 0x95,
        0x00, 0x40, 0x48, 0x00, 0x80, 0xC5, 0x71, 0x24, 0xC9, 0xB2, 0x34, 0xCD, 0xF3, 0x44, 0xD1, 0x34,
        0x4D, 0x93, 0xE4, 0x48, 0x92, 0xA6, 0x79, 0x9E, 0xE7, 0x89, 0xA2, 0x69, 0xAA, 0x2A, 0x49, 0xB2,
        0x2C, 0x4D, 0xF3, 0x3C, 0xCF, 0x13, 0x45, 0xD3, 0x54, 0x55, 0x96, 0x64, 0x59, 0x9A, 0xE6, 0x79,
        0xA2, 0x68, 0x9A, 0xAA, 0xAA, 0xBA, 0xB0, 0x2C, 0x4D, 0xF3, 0x3C, 0x51, 0x34, 0x4D, 0x55, 0x75,
        0x5D, 0x68, 0x9A, 0xA6, 0x89, 0xA2, 0x28, 0x9A, 0xA6, 0xAA, 0xBA, 0x2E, 0x34, 0x4D, 0xF3, 0x44,
        0x51, 0x14, 0x4D, 0x53, 0x55, 0x5D, 0x17, 0x9A, 0xE6, 0x79, 0xA2, 0x68, 0x9A, 0xAA, 0xEA, 0xBA,
        0xB2, 0x0C, 0x3C, 0x4F, 0x14, 0x4D, 0x53, 0x55, 0x5D, 0xD7, 0x75, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x1C,
        0x38, 0x00, 0x00, 0x04, 0x18, 0x41, 0x27, 0x19, 0x55, 0x16, 0x61, 0xA3, 0x09, 0x17, 0x1E, 0x80,
        0x42, 0x43, 0x56, 0x04, 0x00, 0x51, 0x00, 0x00, 0x80, 0x31, 0x88, 0x31, 0xC5, 0x98, 0x61, 0x0A,
        0x4A, 0x29, 0x25, 0x34, 0x8A, 0x41, 0x29, 0x25, 0x94, 0x08, 0x42, 0x48, 0xA9, 0xA4, 0x94, 0x49,
        0x48, 0x2D, 0xB5, 0xD6, 0x32, 0x28, 0x29, 0xB5, 0xD6, 0x5A, 0x25, 0xA5, 0xB4, 0x56, 0x5A, 0xCA,
        0xA4, 0xA4, 0xD6, 0x52, 0x6B, 0x99, 0x94, 0xD4, 0x5A, 0x6B, 0xAD, 0x00, 0x00, 0xB0, 0x03, 0x07,
        0x00, 0xB0, 0x03, 0x0B, 0xA1, 0xD0, 0x90, 0x95, 0x00, 0x40, 0x1E, 0x00, 0x00, 0x83, 0x90, 0x52,
        0x8C, 0x31, 0xC6, 0x18, 0x45, 0x48, 0x29, 0xC6, 0x18, 0x73, 0x8E, 0x22, 0xA4, 0x14, 0x63, 0x8C,
        0x39, 0x47, 0x11, 0x52, 0x8A, 0x31, 0xE7, 0x9C, 0xA3, 0x94, 0x2A, 0xC5, 0x18, 0x73, 0xCE, 0x51,
        0x4A, 0x95, 0x62, 0x8C, 0x39, 0xE7, 0x28, 0xA5, 0x4A, 0x31, 0xC6, 0x98, 0x73, 0x94, 0x52, 0xC6,
        0x18, 0x63, 0xCC, 0x39, 0x4A, 0xA9, 0x94, 0x8C, 0x31, 0xE6, 0x1C, 0xA5, 0x94, 0x52, 0xC6, 0x18,
        0x63, 0x8C, 0x52, 0x4A, 0x29, 0x63, 0x8C, 0x31, 0x26, 0x00, 0x00, 0xA8, 0xC0, 0x01, 0x00, 0x20,
        0xC0, 0x46, 0x91, 0xCD, 0x09, 0x46, 0x82, 0x0A, 0x0D, 0x59, 0x09, 0x00, 0xA4, 0x02, 0x00, 0x38,
        0x1C, 0xC7, 0xB2, 0x34, 0x4D, 0xD3, 0x3C, 0x4F, 0x14, 0x25, 0xC7, 0xB1, 0x2C, 0xCF, 0x13, 0x45,
        0x51, 0x34, 0x4D, 0xCB, 0x71, 0x2C, 0xCB, 0xF3, 0x44, 0x51, 0x14, 0x4D, 0x93, 0x65, 0x69, 0x9A,
        0xE7, 0x89, 0xA2, 0x69, 0xAA, 0x2A, 0xCB, 0xD2, 0x34, 0xCF, 0x13, 0x45, 0xD3, 0x54, 0x55, 0xA6,
        0xE9, 0x79, 0xA2, 0x68, 0x9A, 0xAA, 0xEA, 0xBA, 0x54, 0xD5, 0xF3, 0x44, 0xD1, 0x34, 0x55, 0xD5,
        0x75, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0xE0, 0x09, 0x0E,
        0x00, 0x40, 0x05, 0x36, 0xAC, 0x8E, 0x70, 0x52, 0x34, 0x16, 0x58, 0x68, 0xC8, 0x4A, 0x00, 0x20,
        0x03, 0x00, 0x80, 0x31, 0x06, 0x21, 0x64, 0x0C, 0x42, 0xC8, 0x18, 0x84, 0x10, 0x42, 0x08, 0x21,
        0x84, 0x10, 0x12, 0x00, 0x00, 0x30, 0xE0, 0x00, 0x00, 0x10, 0x60, 0x42, 0x19, 0x28, 0x34, 0x64,
        0x25, 0x00, 0x90, 0x0A, 0x00, 0x40, 0x18, 0xA3, 0x14, 0x63, 0xCE, 0x49, 0x49, 0xA9, 0x32, 0x46,
        0x29, 0xE7, 0x20, 0x94, 0xD2, 0x5A, 0x65, 0x90, 0x52, 0xCE, 0x41, 0x28, 0xA5, 0xB5, 0x66, 0x29,
        0xA5, 0x9C, 0x83, 0x92, 0x52, 0x6B, 0xCD, 0x52, 0x4A, 0x39, 0x27, 0x25, 0xA5, 0xD6, 0x9A, 0x29,
        0x19, 0x83, 0x50, 0x4A, 0x4A, 0xAD, 0x35, 0x95, 0x32, 0x06, 0xA1, 0x94, 0x94, 0x5A, 0x6B, 0xCE,
        0x89, 0x10, 0x42, 0x4A, 0xAD, 0xC5, 0xD8, 0x9C, 0x13, 0x21, 0x84, 0x94, 0x5A, 0x8B, 0xB1, 0x39,
        0x27, 0x63, 0x29, 0x29, 0xB5, 0x18, 0x63, 0x73, 0x4E, 0xC6, 0x52, 0x52, 0x6A, 0x31, 0xC6, 0xE6,
        0x9C, 0x53, 0xAE, 0xB5, 0x16, 0x63, 0xCD, 0x49, 0x29, 0xA5, 0x5C, 0x6B, 0x2D, 0xC6, 0x5A, 0x0B,
        0x00, 0x40, 0x68, 0x70, 0x00, 0x00, 0x3B, 0xB0, 0x61, 0x75, 0x84, 0x93, 0xA2, 0xB1, 0xC0, 0x42,
        0x43, 0x56, 0x02, 0x00, 0x79, 0x00, 0x00, 0x90, 0x52, 0x4A, 0x31, 0xC6, 0x18, 0x63, 0x4C, 0x29,
        0xA5, 0x18, 0x63, 0x8C, 0x31, 0xA6, 0x94, 0x52, 0x8C, 0x31, 0xC6, 0x98, 0x53, 0x4A, 0x29, 0xC6,
        0x18, 0x63, 0xCC, 0x39, 0xA7, 0x14, 0x63, 0x8C, 0x31, 0xE6, 0x9C, 0x63, 0x8C, 0x31, 0xC6, 0x18,
        0x73, 0xCE, 0x31, 0xC6, 0x18, 0x63, 0x8C, 0x39, 0xE7, 0x18, 0x63, 0x8C, 0x31, 0xC6, 0x9C, 0x73,
        0xCE, 0x31, 0xC6, 0x18, 0x63, 0xCE, 0x39, 0xE7, 0x18, 0x63, 0x8C, 0x31, 0xE7, 0x9C, 0x73, 0x8C,
        0x31, 0xC6, 0x98, 0x00, 0x00, 0xA0, 0x02, 0x07, 0x00, 0x80, 0x00, 0x1B, 0x45, 0x36, 0x27, 0x18,
        0x09, 0x2A, 0x34, 0x64, 0x25, 0x00, 0x10, 0x0E, 0x00, 0x00, 0x18, 0xC3, 0x94, 0x73, 0xCE, 0x41,
        0x28, 0x25, 0x95, 0x0A, 0x21, 0xC6, 0x20, 0x74, 0x50, 0x4A, 0x4A, 0xAD, 0x55, 0x08, 0x31, 0x06,
        0x21, 0x84, 0x52, 0x52, 0x6A, 0x2D, 0x6A, 0xCE, 0x39, 0x08, 0x21, 0x94, 0x92, 0x52, 0x6B, 0xD1,
        0x73, 0xCE, 0x41, 0x08, 0xA1, 0x94, 0x94, 0x5A, 0x8B, 0xAA, 0x85, 0x50, 0x4A, 0x29, 0x25, 0xA5,
        0xD6, 0x5A, 0x74, 0x2D, 0x74, 0x52, 0x4A, 0x49, 0xA9, 0xB5, 0x18, 0xA3, 0x94, 0x22, 0x84, 0x90,
        0x52, 0x4A, 0xAD, 0xB5, 0x18, 0x9D, 0x13, 0x21, 0x84, 0x92, 0x52, 0x6A, 0x2D, 0xC6, 0xE6, 0x9C,
        0x8C, 0xA5, 0xA4, 0xD4, 0x5A, 0x8C, 0x31, 0x36, 0xE7, 0x64, 0x2C, 0x25, 0xA5, 0xD6, 0x62, 0x8C,
        0xB1, 0x39, 0xE7, 0x9C, 0x6B, 0xAD, 0xB5, 0x16, 0x63, 0xAD, 0xCD, 0x39, 0xE7, 0x5C, 0x6B, 0x29,
        0xB6, 0x18, 0x6B, 0x6D, 0xCE, 0x39, 0xA7, 0x7B, 0x6C, 0x31, 0xD6, 0x58, 0x6B, 0x73, 0xCE, 0x39,
        0x9F, 0x5B, 0x8B, 0xAD, 0xC6, 0x5A, 0x0B, 0x00, 0x30, 0x79, 0x70, 0x00, 0x80, 0x4A, 0xB0, 0x71,
        0x86, 0x95, 0xA4, 0xB3, 0xC2, 0xD1, 0xE0, 0x42, 0x43, 0x56, 0x02, 0x00, 0xB9, 0x01, 0x00, 0x8C,
        0x52, 0x8C, 0x31, 0xE6, 0x9C, 0x73, 0xCE, 0x39, 0xE7, 0x9C, 0x73, 0xCE, 0x49, 0xA5, 0x18, 0x73,
        0xCE, 0x39, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x94, 0x4A, 0x31, 0xE6, 0x9C, 0x73, 0x10,
        0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x28, 0x19, 0x73, 0xCE, 0x39, 0x07, 0x21, 0x84, 0x10, 0x42,
        0x08, 0x21, 0x84, 0x50, 0x4A, 0xE9, 0x9C, 0x73, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08,
        0xA1, 0x94, 0xD2, 0x39, 0xE7, 0x20, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x29, 0xA5,
        0x73, 0xCE, 0x41, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x52, 0x4A, 0x08, 0x21, 0x84,
        0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0xA5, 0x94, 0x52, 0x42, 0x08, 0x21, 0x84, 0x10,
        0x42, 0x08, 0x21, 0x84, 0x10, 0x4A, 0x29, 0xA5, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42,
        0x08, 0x21, 0x94, 0x52, 0x4A, 0x09, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x28,
        0xA5, 0x94, 0x12, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x25, 0x84, 0x50, 0x4A, 0x29, 0xA5,
        0x94, 0x10, 0x42, 0x08, 0xA1, 0x84, 0x10, 0x42, 0x08, 0xA1, 0x94, 0x52, 0x4A, 0x29, 0x21, 0x84,
        0x52, 0x4A, 0x29, 0x21, 0x84, 0x10, 0x42, 0x29, 0xA5, 0x94, 0x52, 0x42, 0x28, 0xA1, 0x84, 0x10,
        0x42, 0x08, 0x21, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x12, 0x42, 0x29, 0x21, 0x84, 0x10, 0x42,
        0x08, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x42, 0x08, 0x21, 0x84, 0x10, 0x4A, 0x29,
        0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x84, 0x50, 0x42, 0x08, 0x21, 0x94, 0x52, 0x4A, 0x29, 0xA5,
        0x94, 0x52, 0x42, 0x28, 0x25, 0x84, 0x12, 0x42, 0x28, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x84,
        0x50, 0x42, 0x08, 0x21, 0x84, 0x50, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x09, 0x21, 0x84, 0x12,
        0x42, 0x08, 0xA1, 0x00, 0x00, 0xA0, 0x03, 0x07, 0x00, 0x80, 0x00, 0x23, 0x2A, 0x2D, 0xC4, 0x4E,
        0x33, 0xAE, 0x3C, 0x02, 0x47, 0x14, 0x32, 0x4C, 0x40, 0x85, 0x86, 0xAC, 0x04, 0x00, 0xD2, 0x02,
        0x00, 0x00, 0x43, 0xAC, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0x0D, 0x52, 0xD6,
        0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0x46, 0x29, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A,
        0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xA9, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B,
        0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD,
        0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5,
        0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6,
        0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A,
        0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B,
        0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD,
        0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x6B, 0xAD, 0xB5,
        0xD6, 0x5A, 0x6B, 0xAD, 0xB5, 0xD6, 0x5A, 0x4B, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94,
        0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52,
        0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A,
        0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29,
        0xA5, 0x94, 0x52, 0x01, 0xD8, 0x05, 0x1B, 0x0E, 0x80, 0xD1, 0x13, 0x46, 0x12, 0x52, 0x67, 0x19,
        0x56, 0x1A, 0x71, 0xE3, 0x09, 0x18, 0x22, 0x90, 0x42, 0x43, 0x56, 0x02, 0x00, 0x69, 0x01, 0x00,
        0x80, 0x31, 0x8C, 0x31, 0xE6, 0x18, 0x74, 0x10, 0x4A, 0x49, 0x29, 0xA5, 0x0A, 0x21, 0xE7, 0x20,
        0x84, 0x4E, 0x42, 0x2A, 0xAD, 0xC5, 0x16, 0x63, 0x84, 0x90, 0x73, 0x10, 0x42, 0x28, 0x25, 0xA5,
        0xD6, 0x62, 0x8B, 0x31, 0x78, 0x0E, 0x42, 0x08, 0x21, 0x94, 0xD2, 0x52, 0x6C, 0x31, 0xC6, 0x58,
        0x3C, 0x07, 0x21, 0x84, 0x10, 0x52, 0x6A, 0x2D, 0xC6, 0x18, 0x63, 0x0C, 0xB2, 0x85, 0x50, 0x4A,
        0x29, 0x29, 0xB5, 0xD6, 0x62, 0x8C, 0xB5, 0x16, 0xD9, 0x42, 0x28, 0xA5, 0x94, 0x94, 0x5A, 0x8B,
        0x31, 0xD6, 0x5A, 0x83, 0x31, 0xA6, 0x94, 0x92, 0x52, 0x6A, 0xAD, 0xD5, 0x58, 0x63, 0xAC, 0xC5,
        0x18, 0x13, 0x4A, 0x48, 0xA9, 0xB5, 0xD6, 0x62, 0xCC, 0xB5, 0xD6, 0x62, 0x7C, 0xAC, 0x25, 0xA5,
        0xD4, 0x62, 0x8C, 0xB1, 0xC6, 0x58, 0x6B, 0x31, 0xC6, 0xB6, 0x14, 0x52, 0x89, 0x2D, 0xC6, 0x58,
        0x6B, 0x8D, 0xB5, 0x18, 0x61, 0x8C, 0x6A, 0xAD, 0xC5, 0x58, 0x63, 0xAD, 0xB1, 0xD6, 0x5A, 0x8C,
        0x31, 0xC2, 0x95, 0x16, 0x62, 0x8A, 0xB5, 0xD6, 0x5A, 0x73, 0x2D, 0x46, 0x08, 0x63, 0x73, 0x8B,
        0x31, 0xD6, 0x58, 0x6B, 0xAE, 0xB9, 0x16, 0x61, 0x8C, 0xD1, 0xB9, 0x95, 0x5A, 0x6A, 0x8D, 0xB1,
        0xD6, 0x5A, 0x8B, 0x2F, 0xC6, 0x18, 0x61, 0x6B, 0xAC, 0x35, 0xC6, 0x5A, 0x6B, 0xCE, 0xC5, 0x18,
        0x23, 0x84, 0xB0, 0xB5, 0xB6, 0x1A, 0x6B, 0xCD, 0x35, 0xD7, 0x62, 0x8C, 0x31, 0xC6, 0x08, 0x1F,
        0x63, 0xAC, 0xB5, 0xD6, 0xDC, 0x73, 0x31, 0xC6, 0x18, 0x63, 0x84, 0x90, 0x31, 0xC6, 0x1A, 0x6B,
        0xCE, 0xB9, 0x00, 0x80, 0xDC, 0x08, 0x07, 0x00, 0xC4, 0x05, 0x23, 0x09, 0xA9, 0xB3, 0x0C, 0x2B,
        0x8D, 0xB8, 0xF1, 0x04, 0x0C, 0x11, 0x48, 0xA1, 0x21, 0xAB, 0x00, 0x80, 0x18, 0x00, 0x80, 0x21,
        0x00, 0x84, 0x62, 0xB2, 0x01, 0x00, 0x80, 0x09, 0x0E, 0x00, 0x00, 0x01, 0x56, 0xB0, 0x2B, 0xB3,
        0xB4, 0x6A, 0xA3, 0xB8, 0xA9, 0x93, 0xBC, 0xE8, 0x83, 0xC0, 0x27, 0x74, 0xC4, 0x66, 0x64, 0xC8,
        0xA5, 0x54, 0xCC, 0xE4, 0x44, 0xD0, 0x23, 0x35, 0xD4, 0x62, 0x25, 0xD8, 0xA1, 0x15, 0xDC, 0xE0,
        0x05, 0x60, 0xA1, 0x21, 0x2B, 0x01, 0x00, 0x32, 0x00, 0x00, 0xC4, 0x59, 0xCD, 0x39, 0xC7, 0x9C,
        0x2B, 0xE4, 0xA4, 0xB5, 0xD8, 0x6A, 0x2C, 0x15, 0x52, 0x0E, 0x52, 0x8A, 0x31, 0x76, 0xC8, 0x20,
        0xE5, 0x24, 0xC5, 0x5A, 0x32, 0x64, 0x10, 0x83, 0xD4, 0x62, 0xEA, 0x14, 0x32, 0x88, 0x41, 0x6A,
        0xA9, 0x74, 0x0C, 0x19, 0x04, 0x25, 0xC6, 0x54, 0x3A, 0x85, 0x0C, 0x83, 0x5C, 0x63, 0x2B, 0xA1,
        0x63, 0x0E, 0x5A, 0xAB, 0xB1, 0xA5, 0x12, 0x3A, 0x08, 0x00, 0x00, 0x80, 0x20, 0x00, 0xC0, 0x40,
        0x84, 0xCC, 0x04, 0x02, 0x05, 0x50, 0x60, 0x20, 0x03, 0x00, 0x0E, 0x10, 0x12, 0xA4, 0x00, 0x80,
        0xC2, 0x02, 0x43, 0xC7, 0x70, 0x11, 0x10, 0x90, 0x4B, 0xC8, 0x28, 0x30, 0x28, 0x1C, 0x13, 0xCE,
        0x49, 0xA7, 0x0D, 0x00, 0x40, 0x10, 0x22, 0x33, 0x44, 0x22, 0x62, 0x31, 0x48, 0x4C, 0xA8, 0x06,
        0x8A, 0x8A, 0xE9, 0x00, 0x60, 0x71, 0x81, 0x21, 0x1F, 0x00, 0x32, 0x34, 0x36, 0xD2, 0x2E, 0x2E,
        0xA0, 0xCB, 0x00, 0x17, 0x74, 0x71, 0xD7, 0x81, 0x10, 0x82, 0x10, 0x84, 0x20, 0x16, 0x07, 0x50,
        0x40, 0x02, 0x0E, 0x4E, 0xB8, 0xE1, 0x89, 0x37, 0x3C, 0xE1, 0x06, 0x27, 0xE8, 0x14, 0x95, 0x3A,
        0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x78, 0x00, 0x00, 0x48, 0x36, 0x80, 0x88, 0x68,
        0x66, 0xE6, 0x38, 0x3A, 0x3C, 0x3E, 0x40, 0x42, 0x44, 0x46, 0x48, 0x4A, 0x4C, 0x4E, 0x50, 0x52,
        0x54, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x3E, 0x00, 0x00, 0x92, 0x15, 0x20, 0x22,
        0x9A, 0x99, 0x39, 0x8E, 0x0E, 0x8F, 0x0F, 0x90, 0x10, 0x91, 0x11, 0x92, 0x12, 0x93, 0x13, 0x94,
        0x14, 0x95, 0x00, 0x00, 0x40, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x10, 0x40, 0x00, 0x02, 0x02,
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x02
    ];
    let test_header = &test_header[..];

    let mut stream_id = 0u32;
    let (vorbis_identification_header, vorbis_comment_header, vorbis_setup_header) = parse_vorbis_headers(test_header, &mut stream_id).unwrap();

    use crate::ogg::OggStreamWriter;
    use crate::io_utils::SharedCursor;
    let data = SharedCursor::new();
    let mut ogg_stream_writer = OggStreamWriter::new(data.clone(), stream_id);
    ogg_stream_writer.write_all(&vorbis_identification_header.to_packed().unwrap().data).unwrap();
    ogg_stream_writer.seal_packet(0, false).unwrap();
    ogg_stream_writer.write_all(&vorbis_comment_header.to_packed().unwrap().data).unwrap();
    ogg_stream_writer.write_all(&vorbis_setup_header.to_packed().unwrap().data).unwrap();
    ogg_stream_writer.seal_packet(0, false).unwrap();
    ogg_stream_writer.flush().unwrap();
    let new_bytes = &data.get_vec();

    dbg!(vorbis_identification_header);
    dbg!(vorbis_comment_header);
    dbg!(vorbis_setup_header);

    let (vorbis_identification_header_2, vorbis_comment_header_2, vorbis_setup_header_2) = parse_vorbis_headers(new_bytes, &mut stream_id).unwrap();

    dbg!(vorbis_identification_header_2);
    dbg!(vorbis_comment_header_2);
    dbg!(vorbis_setup_header_2);

    assert_eq!(test_header, new_bytes);
}
