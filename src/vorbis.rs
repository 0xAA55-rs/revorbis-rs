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
use crate::io_utils::CursorVecU8;
use crate::utils::BitwiseData;

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

const SHOW_DEBUG: bool = false;
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

fn ilog(mut v: u32) -> i32 {
    let mut ret = 0;
    while v != 0 {
        v >>= 1;
        ret += 1;
    }
    ret
}

fn icount(mut v: u32) -> u32 {
    let mut ret = 0u32;
    while v != 0 {
        ret += v & 1;
        v >>= 1;
    }
    ret
}

/// * BitReader: read vorbis data bit by bit
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
pub struct BitWriter {
    /// * Currently ends at which bit in the last byte
    pub endbit: i32,

    /// * How many bits did we wrote in total
    pub total_bits: usize,

    /// * We owns the written data
    pub cursor: CursorVecU8,
}

impl Default for BitWriter {
    fn default() -> Self {
        // We must have at least one byte written here because we have to add bits to the last byte.
        let mut cursor = CursorVecU8::new(vec![0]);
        cursor.seek(SeekFrom::End(0)).unwrap();
        Self {
            endbit: 0,
            total_bits: 0,
            cursor,
        }
    }
}

impl BitWriter {
    /// * Create a `CursorVecU8` to write
    pub fn new() -> Self {
        Self::default()
    }

    /// * Get the last byte for modifying it
    pub fn last_byte(&mut self) -> &mut u8 {
        let v = self.cursor.get_mut();
        let len = v.len();
        &mut v[len - 1]
    }

    /// * Write data by bytes one by one
    fn write_byte(&mut self, byte: u8) -> Result<(), AudioWriteError> {
        self.cursor.write_all(&[byte])?;
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

    /// * Get the inner byte array and consumes the writer.
    pub fn into_bytes(self) -> Vec<u8> {
        self.cursor.into_inner()
    }
}

/// * This is the parsed Vorbis codebook, it's used to quantify the audio samples.
/// * This is the re-invented wheel. For this piece of code, this thing is only used to parse the binary form of the codebooks.
/// * And then I can sum up how many **bits** were used to store the codebooks.
/// * Vorbis data are all stored in bitwise form, almost anything is not byte-aligned. Split data in byte arrays just won't work on Vorbis data.
/// * We have to do it in a bitwise way.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct CodeBook {
    pub dim: u16,
    pub entries: u32,
    pub lengthlist: Vec<i8>,
    pub maptype: u32,
    pub q_min: isize,
    pub q_delta: isize,
    pub q_quant: i32,
    pub q_sequencep: i32,
    pub quantlist: Vec<i16>,
}

impl Debug for CodeBook {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("CodeBook")
        .field("dim", &self.dim)
        .field("entries", &self.entries)
        .field("lengthlist", &format_args!("[{}]", format_array!(self.lengthlist, " ", "0x{:02x}")))
        .field("maptype", &self.maptype)
        .field("q_min", &self.q_min)
        .field("q_delta", &self.q_delta)
        .field("q_quant", &self.q_quant)
        .field("q_sequencep", &self.q_sequencep)
        .field("quantlist", &format_args!("[{}]", format_array!(self.quantlist, " ", "0x{:04x}")))
        .finish()
    }
}

impl CodeBook {
    /// unpacks a codebook from the packet buffer into the codebook struct,
    /// readies the codebook auxiliary structures for decode
    pub fn read(bitreader: &mut BitReader) -> Result<Self, AudioReadError> {
        let mut ret = Self::default();
        ret.parse_book(bitreader)?;
        Ok(ret)
    }

    fn parse_book(&mut self, bitreader: &mut BitReader) -> Result<(), AudioReadError> {
        /* make sure alignment is correct */
        if bitreader.read(24)? != 0x564342 {
            return Err(AudioReadError::FormatError("Check the `BCV` flag failed.".to_string()));
        }
        /* first the basic parameters */
        let dim = bitreader.read(16)?;
        let entries = bitreader.read(24)?;
        if ilog(dim as u32) + ilog(entries as u32) > 24 {
            return Err(AudioReadError::FormatError(format!("{} + {} > 24", ilog(dim as u32), ilog(entries as u32))));
        }
        self.dim = dim as u16;
        self.entries = entries as u32;
        /* codeword ordering.... length ordered or unordered? */
        match bitreader.read(1)? {
            0 => {
                debugln!("  unordered");

                /* allocated but unused entries? */
                let unused = bitreader.read(1)? != 0;

                /* unordered */
                self.lengthlist.resize(self.entries as usize, 0);
                /* allocated but unused entries? */
                if unused {
                    /* yes, unused entries */
                    debugln!("  with unused entries");

                    for i in 0..self.entries as usize {
                        if bitreader.read(1)? != 0 {
                            let num = bitreader.read(5)? as i8;
                            self.lengthlist[i] = num + 1;
                        } else {
                            self.lengthlist[i] = 0;
                        }
                    }
                } else { /* all entries used; no tagging */
                    for i in 0..self.entries as usize {
                        let num = bitreader.read(5)? as i8;
                        self.lengthlist[i] = num + 1;
                    }
                }
            }
            1 => { /* ordered */
                debugln!("  ordered");

                let mut length = (bitreader.read(5)? + 1) as i8;
                self.lengthlist.resize(self.entries as usize, 0);
                let mut i = 0;
                while i < self.entries {
                    let num = bitreader.read(ilog(self.entries - i))? as u32;
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

        debugln!("  lengthlist: [{}]", format_array!(&self.lengthlist, ", ", "{:02}"));

        /* Do we have a mapping to unpack? */
        self.maptype = bitreader.read(4)? as u32;
        debugln!("  maptype: {}", self.maptype);
        match self.maptype {
            0 => (),
            1 | 2 => {
                /* implicitly populated value mapping */
                /* explicitly populated value mapping */
                self.q_min = bitreader.read(32)? as isize;
                self.q_delta = bitreader.read(32)? as isize;
                self.q_quant = bitreader.read(4)? + 1;
                self.q_sequencep = bitreader.read(1)?;

                debugln!("    q_min: {}", self.q_min);
                debugln!("    q_delta: {}", self.q_delta);
                debugln!("    q_quant: {}", self.q_quant);
                debugln!("    q_sequencep: {}", self.q_sequencep);

                let quantvals = match self.maptype {
                    1 => if self.dim == 0 {0} else {self.book_maptype1_quantvals() as usize},
                    2 => self.entries as usize * self.dim as usize,
                    _ => unreachable!(),
                };

                debugln!("    quantvals: {quantvals}");

                /* quantized values */
                self.quantlist.resize(quantvals, 0);
                for i in 0..quantvals {
                    self.quantlist[i] = bitreader.read(self.q_quant)? as i16;
                }

                debugln!("    quantlist: [{}]", format_array!(&self.quantlist, ", ", "0x{:04}"));
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
        use std::cmp::max;
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

    /// * Pack the book into the bitstream
    pub fn write(&self, bitwriter: &mut BitWriter) -> Result<(), AudioWriteError> {
        /* first the basic parameters */
        bitwriter.write(0x564342, 24)?;
        bitwriter.write(self.dim as u32, 16)?;
        bitwriter.write(self.entries, 24)?;

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
            let mut count = 0u32;
            bitwriter.write(1, 1)?; /* ordered */
            bitwriter.write(self.lengthlist[0] as u32 - 1, 5)?;

            for i in 1..self.entries as usize {
                let this = self.lengthlist[i];
                let last = self.lengthlist[i - 1];
                if this > last {
                    for _ in last..this {
                        bitwriter.write(i as u32 - count, ilog(self.entries - count))?;
                        count = i as u32;
                    }
                }
            }
            bitwriter.write(self.entries - count, ilog(self.entries - count))?;
        } else {
            /* length random.  Again, we don't code the codeword itself, just
               the length.  This time, though, we have to encode each length */
            bitwriter.write(0, 1)?; /* unordered */

            /* algortihmic mapping has use for 'unused entries', which we tag
               here.  The algorithmic mapping happens as usual, but the unused
               entry has no codeword. */
            let mut i = 0u32;
            while i < self.entries {
                if self.lengthlist[i as usize] == 0 {
                    break;
                }
                i += 1;
            }

            if i == self.entries {
                bitwriter.write(0, 1)?; /* no unused entries */
                for i in 0..self.entries as usize {
                    bitwriter.write(self.lengthlist[i] as u32 - 1, 5)?;
                }
            } else {
                bitwriter.write(1, 1)?; /* we have unused entries; thus we tag */
                for i in 0..self.entries as usize {
                    if self.lengthlist[i] == 0 {
                        bitwriter.write(0, 1)?;
                    } else {
                        bitwriter.write(1, 1)?;
                        bitwriter.write(self.lengthlist[i] as u32 - 1, 5)?;
                    }
                }
            }
        }

        /* is the entry number the desired return value, or do we have a
           mapping? If we have a mapping, what type? */
        bitwriter.write(self.maptype, 4)?;
        match self.maptype {
            0 => (),
            1 | 2 => {
                if self.quantlist.is_empty() {
                    return Err(AudioWriteError::MissingData("Missing quantlist data".to_string()));
                }

                bitwriter.write(self.q_min as u32, 32)?;
                bitwriter.write(self.q_delta as u32, 32)?;
                bitwriter.write((self.q_quant - 1) as u32, 4)?;
                bitwriter.write(self.q_sequencep as u32, 1)?;

                let quantvals = match self.maptype {
                    1 => self.book_maptype1_quantvals() as usize,
                    2 => self.entries as usize * self.dim as usize,
                    _ => unreachable!(),
                };

                for i in 0..quantvals {
                    bitwriter.write(self.quantlist[i].unsigned_abs() as u32, self.q_quant)?;
                }
            }
            o => return Err(AudioWriteError::InvalidData(format!("Unexpected maptype {o}"))),
        }

        Ok(())
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
        CodeBooks::load(&self.books.data)
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
    pub fn load(data: &[u8]) -> Result<Self, AudioReadError> {
        let mut bitreader = BitReader::new(data);
        let num_books = (bitreader.read(8)? + 1) as usize;
        let mut books = Vec::<CodeBook>::with_capacity(num_books);
        let mut bits_of_books = Vec::<usize>::with_capacity(num_books);
        for i in 0..num_books {
            debugln!("Reading codebook {i}");
            let cur_bit_pos = bitreader.total_bits;
            books.push(CodeBook::read(&mut bitreader)?);
            bits_of_books.push(bitreader.total_bits - cur_bit_pos);
        }
        Ok(Self {
            books,
            bits_of_books,
            total_bits: bitreader.total_bits,
        })
    }

    /// * Get the total bits of the codebook.
    pub fn get_total_bits(&self) -> usize {
        self.total_bits
    }

    /// * Get the total bytes of the codebook that are able to contain all of the bits.
    pub fn get_total_bytes(&self) -> usize {
        BitwiseData::calc_total_bytes(self.total_bits)
    }

    /// * Pack the codebook to binary for storage.
    pub fn pack(&self) -> Result<CodeBooksPacked, AudioWriteError> {
        let mut bitwriter = BitWriter::new();
        let mut bits_of_books = Vec::<usize>::with_capacity(self.books.len());
        bitwriter.write((self.books.len().wrapping_sub(1)) as u32, 8)?;
        for book in self.books.iter() {
            let cur_bit_pos = bitwriter.total_bits;
            book.write(&mut bitwriter)?;
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

impl From<CodeBooksPacked> for CodeBooks {
    fn from(packed: CodeBooksPacked) -> Self {
        let ret = Self::load(&packed.books.data).unwrap();
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

macro_rules! read_slice {
    ($data:ident, $length:expr) => {
        {
            let ret = &$data[..$length];
            $data = &$data[$length..];
            ret
        }
    }
}

macro_rules! read_slice_4 {
    ($data:ident) => {
        {
            let ret = &$data[..4];
            $data = &$data[4..];
            [ret[0], ret[1], ret[2], ret[3]]
        }
    }
}

macro_rules! read_string {
    ($data:ident, $length:expr) => {
        {
            let s = read_slice!($data, $length);
            match std::str::from_utf8(s) {
                Ok(s) => Ok(s.to_string()),
                Err(_) => Err(AudioError::InvalidData(format!("Parse UTF-8 failed: {}", String::from_utf8_lossy(s)))),
            }
        }
    }
}

/// * The `VorbisIdentificationHeader` is the Vorbis identification header, the first header
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct VorbisIdentificationHeader {
    pub version: u32,
    pub channels: u8,
    pub sample_rate: u32,
    pub bitrate_upper: u32,
    pub bitrate_nominal: u32,
    pub bitrate_lower: u32,
    pub block_size: [u16; 2],
    pub framing_flag: bool,
}

impl VorbisIdentificationHeader {
    pub fn parse(data: &[u8]) -> Result<Self, AudioError> {
        let mut data = data;
        let ident = read_slice!(data, 7);
        if ident != b"\x01vorbis" {
            Err(AudioError::InvalidData(format!("Not a Vorbis identification header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let version = u32::from_le_bytes(read_slice_4!(data));
            let channels = read_slice!(data, 1)[0];
            let sample_rate = u32::from_le_bytes(read_slice_4!(data));
            let bitrate_upper = u32::from_le_bytes(read_slice_4!(data));
            let bitrate_nominal = u32::from_le_bytes(read_slice_4!(data));
            let bitrate_lower = u32::from_le_bytes(read_slice_4!(data));
            let bs = read_slice!(data, 1)[0];
            let bs_1 = bs & 0x0F;
            let bs_2 = bs >> 4;
            let block_size = [1 << bs_1, 1 << bs_2];
            let framing_flag = read_slice!(data, 1)[0] & 1 == 1;
            if !data.is_empty()
            || sample_rate < 1
            || channels < 1
            || block_size[0] < 64
            || block_size[1] < block_size[0]
            || block_size[1] > 8192
            || !framing_flag {
                Err(AudioError::InvalidData("Bad Vorbis identification header.".to_string()))
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

    pub fn pack(&self) -> Vec<u8> {
        let bs_1 = ilog((self.block_size[0] - 1) as u32) as u8;
        let bs_2 = ilog((self.block_size[1] - 1) as u32) as u8;
        [
            b"\x01vorbis" as &[u8],
            &self.version.to_le_bytes() as &[u8],
            &[self.channels],
            &self.sample_rate.to_le_bytes() as &[u8],
            &self.bitrate_upper.to_le_bytes() as &[u8],
            &self.bitrate_nominal.to_le_bytes() as &[u8],
            &self.bitrate_lower.to_le_bytes() as &[u8],
            &[bs_1 | (bs_2 << 4)],
            &[if self.framing_flag {1} else {0}],
        ].into_iter().flatten().copied().collect()
    }
}

/// * The `VorbisCommentHeader` is the Vorbis comment header, the second header
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct VorbisCommentHeader {
    comments: Vec<String>,
    vendor: String,
}

impl VorbisCommentHeader {
    pub fn parse(data: &[u8]) -> Result<Self, AudioError> {
        let mut data = data;
        let ident = read_slice!(data, 7);
        if ident != b"\x03vorbis" {
            Err(AudioError::InvalidData(format!("Not a Vorbis comment header, the header type is {}, the string is {}", ident[0], String::from_utf8_lossy(&ident[1..]))))
        } else {
            let vendor_len = i32::from_le_bytes(read_slice_4!(data));
            if vendor_len < 0 || vendor_len as usize > data.len() {
                return Err(AudioError::InvalidData(format!("Bad vendor string length {vendor_len}")));
            }
            let vendor = read_string!(data, vendor_len as usize)?;
            let num_comments = i32::from_le_bytes(read_slice_4!(data)) as usize;
            let mut comments = Vec::<String>::with_capacity(num_comments);
            for _ in 0..num_comments {
                let comment_len = i32::from_le_bytes(read_slice_4!(data));
                if comment_len < 0 || comment_len as usize > data.len() {
                    return Err(AudioError::InvalidData(format!("Bad comment string length {vendor_len}")));
                }
                comments.push(read_string!(data, comment_len as usize)?);
            }
            Ok(Self{
                comments,
                vendor,
            })
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        [
            b"\x03vorbis" as &[u8],
            &((self.vendor.len() as u32).to_le_bytes()) as &[u8],
            self.vendor.as_bytes() as &[u8],
        ].into_iter().flatten().copied().chain(self.comments.iter().flat_map(|comment| -> Vec<u8> {
            [
                &((comment.len() as u32).to_le_bytes()) as &[u8],
                comment.as_bytes() as &[u8],
            ].into_iter().flatten().copied().collect()
        })).collect()
    }
}



/// * This function extracts data from an Ogg packet, the packet contains the Vorbis header.
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

/// * This function removes the codebooks from the Vorbis setup header. The setup header was extracted from the Ogg stream.
/// * Since Vorbis stores data in bitwise form, all of the data are not aligned in bytes, we have to parse it bit by bit.
/// * After parsing the codebooks, we can sum up the total bits of the codebooks, and then we can replace it with an empty codebook.
/// * At last, use our `BitwiseData` to concatenate these bit-strings without any gaps.
pub fn remove_codebook_from_setup_header(setup_header: &[u8]) -> Result<Vec<u8>, AudioError> {
    // Try to verify if this is the right way to read the codebook
    assert_eq!(&setup_header[0..7], b"\x05vorbis", "Checking the vorbis header that is a `setup_header` or not");

    // Let's find the book, and kill it.
    let codebooks = CodeBooks::load(&setup_header[7..]).unwrap();
    let bytes_before_codebook = BitwiseData::from_bytes(&setup_header[0..7]);
    let (_codebook_bits, bits_after_codebook) = BitwiseData::new(&setup_header[7..], (setup_header.len() - 7) * 8).split(codebooks.total_bits);

    // Let's generate the empty codebook.
    let _empty_codebooks = CodeBooks::default().pack().unwrap().books;

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
