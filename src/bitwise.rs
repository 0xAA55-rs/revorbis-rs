use std::{
    io::{self, Write},
    fmt::{self, Debug, Formatter},
};

use crate::*;
use io_utils::{Writer, CursorVecU8};

const MASK8: [u8; 9] = [0x00, 0x01, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0xFF];

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

macro_rules! define_worksize_consts {
    () => {
        const BITS: usize = Unit::BITS as usize;
        const ALIGN: usize = BITS / 8;
    }
}

macro_rules! define_worksize {
    (8) => {
        type  Unit = u8;
        define_worksize_consts!();
    };
    (16) => {
        type  Unit = u16;
        define_worksize_consts!();
    };
    (32) => {
        type  Unit = u32;
        define_worksize_consts!();
    };
    (64) => {
        type  Unit = u64;
        define_worksize_consts!();
    };
}

define_worksize!(8);

#[macro_export]
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

#[macro_export]
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
    pub fn read(&mut self, mut bits: i32) -> io::Result<i32> {
        if !(0..=32).contains(&bits) {
            return_Err!(io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid bit number: {bits}")));
        }
        let mut ret: i32;
        let m = MASK[bits as usize];
        let origbits = bits;
        let cursor = self.cursor;

        // Don't want it panic, and don't want an Option.
        let ptr_index = |mut index: usize| -> io::Result<u8> {
            index += cursor;
            let eof_err = || -> io::Error {
                io::Error::new(io::ErrorKind::UnexpectedEof, format!("UnexpectedEof when trying to read {origbits} bits from the input position 0x{:x}", index))
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
#[derive(Default, Debug)]
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
    fn write_byte(&mut self, byte: u8) -> io::Result<()> {
        self.cache.write_all(&[byte])?;
        if self.cache.len() >= Self::CACHE_SIZE {
            self.flush()?;
        }
        Ok(())
    }

    /// * Write data in bits, max is 32 bit.
    pub fn write(&mut self, mut value: u32, mut bits: i32) -> io::Result<()> {
        if !(0..=32).contains(&bits) {
            return_Err!(io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid bits {bits}")));
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

    pub fn flush(&mut self) -> io::Result<()> {
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

    pub fn force_flush(&mut self) -> io::Result<()> {
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

/// * Read bits of data using `BitReader`
#[macro_export]
macro_rules! read_bits {
    ($bitreader:ident, $bits:expr) => {
        if DEBUG_ON_READ_BITS {
            $bitreader.read($bits).unwrap()
        } else {
            $bitreader.read($bits)?
        }
    };
}

/// * Read a `f32` using `BitReader`
#[macro_export]
macro_rules! read_f32 {
    ($bitreader:ident) => {
        unsafe {std::mem::transmute::<_, f32>(read_bits!($bitreader, 32))}
    };
}

/// * Write bits of data using `BitWriter<W>`
#[macro_export]
macro_rules! write_bits {
    ($bitwriter:ident, $data:expr, $bits:expr) => {
        if DEBUG_ON_WRITE_BITS {
            $bitwriter.write($data as u32, $bits).unwrap()
        } else {
            $bitwriter.write($data as u32, $bits)?
        }
    };
}

/// * Write a `f32` using `BitWriter<W>`
#[macro_export]
macro_rules! write_f32 {
    ($bitwriter:ident, $data:expr) => {
        write_bits!($bitwriter, unsafe {std::mem::transmute::<_, u32>($data)}, 32)
    };
}

/// * Read a byte array `slice` using the `BitReader`
#[macro_export]
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
#[macro_export]
macro_rules! read_string {
    ($bitreader:ident, $length:expr) => {
        {
            let s = read_slice!($bitreader, $length);
            match std::str::from_utf8(&s) {
                Ok(s) => Ok(s.to_string()),
                Err(_) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("Parse UTF-8 failed: {}", String::from_utf8_lossy(&s)))),
            }
        }
    };
}

/// * Write a slice to the `BitWriter`
#[macro_export]
macro_rules! write_slice {
    ($bitwriter:ident, $data:expr) => {
        for &data in $data.iter() {
            write_bits!($bitwriter, data, std::mem::size_of_val(&data) as i32 * 8);
        }
    };
}

/// * Write a sized string to the `BitWriter`
#[macro_export]
macro_rules! write_string {
    ($bitwriter:ident, $string:expr) => {
        write_slice!($bitwriter, $string.as_bytes());
    };
}

/// * Alignment calculation
pub fn align(size: usize, alignment: usize) -> usize {
    if size != 0 {
        ((size - 1) / alignment + 1) * alignment
    } else {
        0
    }
}

/// * Transmute vector, change its type, but not by cloning it or changing its memory location or capacity.
/// * Will panic or crash if you don't know what you are doing.
pub fn transmute_vector<S, D>(vector: Vec<S>) -> Vec<D>
where
    S: Sized,
    D: Sized {

    use std::{any::type_name, mem::{size_of, ManuallyDrop}};
    let s_size = size_of::<S>();
    let d_size = size_of::<D>();
    let s_name = type_name::<S>();
    let d_name = type_name::<D>();
    let size_in_bytes = s_size * vector.len();
    let remain_size = size_in_bytes % d_size;
    if remain_size != 0 {
        panic!("Could not transmute from Vec<{s_name}> to Vec<{d_name}>: the number of bytes {size_in_bytes} is not divisible to {d_size}.")
    } else {
        let mut s = ManuallyDrop::new(vector);
        unsafe {
            Vec::<D>::from_raw_parts(s.as_mut_ptr() as *mut D, size_in_bytes / d_size, s.capacity() * s_size / d_size)
        }
    }
}

/// * Shift an array of bits to the front. In a byte, the lower bits are the front bits.
pub fn shift_data_to_front(data: &[u8], bits: usize, total_bits: usize) -> Vec<u8> {
    if bits == 0 {
        data.to_owned()
    } else if bits >= total_bits {
        Vec::new()
    } else {
        let shifted_total_bits = total_bits - bits;
        let mut data = {
            let bytes_moving = bits >> 3;
            data[bytes_moving..].to_vec()
        };
        let bits = bits & 7;
        if bits == 0 {
            data
        } else {
            data.resize(align(data.len(), ALIGN), 0);
            let mut to_shift: Vec<Unit> = transmute_vector(data);

            fn combine_bits(data1: Unit, data2: Unit, bits: usize) -> Unit {
                let move_high = BITS - bits;
                (data1 >> bits) | (data2 << move_high)
            }

            for i in 0..(to_shift.len() - 1) {
                to_shift[i] = combine_bits(to_shift[i], to_shift[i + 1], bits);
            }

            let last = to_shift.pop().unwrap() >> bits;
            to_shift.push(last);

            let mut ret = transmute_vector(to_shift);
            ret.truncate(align(shifted_total_bits, 8) / 8);
            ret
        }
    }
}

/// * Shift an array of bits to the back. In a byte, the higher bits are the back bits.
pub fn shift_data_to_back(data: &[u8], bits: usize, total_bits: usize) -> Vec<u8> {
    if bits == 0 {
        data.to_owned()
    } else {
        let shifted_total_bits = total_bits + bits;
        let data = {
            let bytes_added = align(bits, 8) / 8;
            let data: Vec<u8> = [vec![0u8; bytes_added], data.to_owned()].iter().flatten().copied().collect();
            data
        };
        let bits = bits & 7;
        if bits == 0 {
            data
        } else {
            let lsh = 8 - bits;
            shift_data_to_front(&data, lsh, shifted_total_bits + lsh)
        }
    }
}


/// * A utility for you to manipulate data bitwise, mainly to concatenate data in bits or to split data from a specific bit position.
/// * This is mainly used for Vorbis data parsing.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct BitwiseData {
    /// * Store as bytes
    pub data: Vec<u8>,

    /// * The total bits of the books
    pub total_bits: usize,
}

impl BitwiseData {
    pub fn new(data: &[u8], total_bits: usize) -> Self {
        let mut ret = Self {
            data: data[..Self::calc_total_bytes(total_bits)].to_vec(),
            total_bits,
        };
        ret.remove_residue();
        ret
    }

    /// * Construct from bytes
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            total_bits: data.len() * 8,
        }
    }

    /// * If there are any `1` bits outside of the byte array, erase them to zeros.
    fn remove_residue(&mut self) {
        let residue_bits = self.total_bits & 7;
        if residue_bits == 0 {
            return;
        }
        if let Some(byte) = self.data.pop() { self.data.push(byte & MASK8[residue_bits]) }
    }

    /// * Get the number of total bits in the `data` field
    pub fn get_total_bits(&self) -> usize {
        self.total_bits
    }

    /// * Get the number of bytes that are just enough to contain all of the bits.
    pub fn get_total_bytes(&self) -> usize {
        Self::calc_total_bytes(self.total_bits)
    }

    /// * Get the number of bytes that are just enough to contain all of the bits.
    pub fn calc_total_bytes(total_bits: usize) -> usize {
        align(total_bits, 8) / 8
    }

    /// * Resize to the aligned size. Doing this is for `shift_data_to_front()` and `shift_data_to_back()` to manipulate bits efficiently.
    pub fn fit_to_aligned_size(&mut self) {
        self.data.resize(align(self.total_bits, BITS) / 8, 0);
    }

    /// * Resize to the number of bytes that are just enough to contain all of the bits.
    pub fn shrink_to_fit(&mut self) {
        self.data.truncate(self.get_total_bytes());
        self.remove_residue();
    }

    /// * Check if the data length is just the aligned size.
    pub fn is_aligned_size(&self) -> bool {
        self.data.len() == align(self.data.len(), ALIGN)
    }

    /// * Breakdown to 2 parts of the data at the specific bitvise position.
    pub fn split(&self, split_at_bit: usize) -> (Self, Self) {
        if split_at_bit == 0 {
            (Self::default(), self.clone())
        } else if split_at_bit >= self.total_bits {
            (self.clone(), Self::default())
        } else {
            let data1 = {
                let mut data = self.clone();
                data.total_bits = split_at_bit;
                data.shrink_to_fit();
                let last_bits = data.total_bits & 7;
                if last_bits != 0 {
                    let last_byte = data.data.pop().unwrap();
                    data.data.push(last_byte & MASK8[last_bits]);
                }
                data
            };
            let data2 = Self {
                data: shift_data_to_front(&self.data, split_at_bit, self.total_bits),
                total_bits: self.total_bits - split_at_bit,
            };
            (data1, data2)
        }
    }

    /// * Concat another `BitwiseData` to the bitstream, without the gap.
    pub fn concat(&mut self, rhs: &Self) {
        if rhs.total_bits == 0 {
            return;
        }
        self.shrink_to_fit();
        let shifts = self.total_bits & 7;
        if shifts == 0 {
            self.data.extend(&rhs.data);
        } else {
            let shift_left = 8 - shifts;
            let last_byte = self.data.pop().unwrap();
            self.data.push(last_byte | (rhs.data[0] << shifts));
            self.data.extend(shift_data_to_front(&rhs.data, shift_left, rhs.total_bits));
        }
        self.total_bits += rhs.total_bits;
    }

    /// * Turn to byte array
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.shrink_to_fit();
        self.data
    }
}

impl Debug for BitwiseData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("BitwiseData")
        .field("data", &format_args!("{}", format_array!(self.data, hex2)))
        .field("total_bits", &self.total_bits)
        .finish()
    }
}
