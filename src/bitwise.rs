#![allow(dead_code)]
use std::fmt::{self, Debug, Formatter};
use crate::format_array;

const MASK8: [u8; 9] = [0x00, 0x01, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0xFF];

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
pub fn shift_data_to_front(data: &Vec<u8>, bits: usize, total_bits: usize) -> Vec<u8> {
    if bits == 0 {
        data.clone()
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
pub fn shift_data_to_back(data: &Vec<u8>, bits: usize, total_bits: usize) -> Vec<u8> {
    if bits == 0 {
        data.clone()
    } else {
        let shifted_total_bits = total_bits + bits;
        let data = {
            let bytes_added = align(bits, 8) / 8;
            let data: Vec<u8> = [vec![0u8; bytes_added], data.clone()].iter().flatten().copied().collect();
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


#[derive(Clone, PartialEq, Eq)]
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
        match self.data.pop() {
            Some(byte) => self.data.push(byte & MASK8[residue_bits]),
            None => (),
        }
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

impl Default for BitwiseData {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            total_bits: 0,
        }
    }
}

impl Debug for BitwiseData {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("BitwiseData")
        .field("data", &format_args!("{}", format_array!(self.data, " ", "{:02x}")))
        .field("total_bits", &self.total_bits)
        .finish()
    }
}
