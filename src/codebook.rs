use std::{
    cmp::max,
    fmt::{self, Debug, Formatter},
    io::{self, Write},
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};

use crate::*;
use io_utils::CursorVecU8;

/// * This is the parsed Vorbis codebook, it's used to quantify the audio samples.
/// * This is the re-invented wheel. For this piece of code, this thing is only used to parse the binary form of the codebooks.
/// * And then I can sum up how many **bits** were used to store the codebooks.
/// * Vorbis data are all stored in bitwise form, almost anything is not byte-aligned. Split data in byte arrays just won't work on Vorbis data.
/// * We have to do it in a bitwise way.
#[derive(Default, Clone, PartialEq)]
pub struct StaticCodeBook {
    pub dim: i32,
    pub entries: i32,
    pub lengthlist: Vec<i8>,
    pub maptype: i32,
    pub q_min: f32,
    pub q_delta: f32,
    pub q_quant: i32,
    pub q_sequencep: i32,
    pub quantlist: Vec<i32>,
}

impl Debug for StaticCodeBook {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("StaticCodeBook")
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

impl StaticCodeBook {
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
                ret.q_min = read_f32!(bitreader);
                ret.q_delta = read_f32!(bitreader);
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

impl VorbisPackableObject for StaticCodeBook {
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

                write_f32!(bitwriter, self.q_min);
                write_f32!(bitwriter, self.q_delta);
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
pub struct StaticCodeBooksPacked {
    /// * The packed code books
    pub books: BitwiseData,

    /// * The size of each codebook in bits
    pub bits_of_books: Vec<usize>,
}

impl StaticCodeBooksPacked {
    pub fn unpack(&self) -> Result<StaticCodeBooks, io::Error> {
        StaticCodeBooks::load_from_slice(&self.books.data)
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

#[derive(Default, Clone, PartialEq)]
pub struct StaticCodeBooks {
    /// * The unpacked codebooks
    pub books: Vec<StaticCodeBook>,

    /// * The size of each codebook in bits if they are packed
    pub bits_of_books: Vec<usize>,

    /// * The total bits of all the books
    pub total_bits: usize,
}

impl StaticCodeBooks {
    /// * Unpack the codebooks from the bitstream
    pub fn load(bitreader: &mut BitReader) -> Result<Self, io::Error> {
        let begin_bits = bitreader.total_bits;
        let num_books = (read_bits!(bitreader, 8).wrapping_add(1)) as usize;
        let mut books = Vec::<StaticCodeBook>::with_capacity(num_books);
        let mut bits_of_books = Vec::<usize>::with_capacity(num_books);
        for _ in 0..num_books {
            let cur_bit_pos = bitreader.total_bits;
            books.push(StaticCodeBook::load(bitreader)?);
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
    pub fn to_packed_codebooks(&self) -> Result<StaticCodeBooksPacked, io::Error> {
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
        Ok(StaticCodeBooksPacked{
            books: BitwiseData::new(&books, total_bits),
            bits_of_books,
        })
    }
}

impl VorbisPackableObject for StaticCodeBooks {
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

impl From<StaticCodeBooksPacked> for StaticCodeBooks {
    fn from(packed: StaticCodeBooksPacked) -> Self {
        let ret = Self::load_from_slice(&packed.books.data).unwrap();
        assert_eq!(ret.bits_of_books, packed.bits_of_books, "StaticCodeBooks::from(&StaticCodeBooksPacked), bits_of_books");
        assert_eq!(ret.total_bits, packed.books.total_bits, "StaticCodeBooks::from(&StaticCodeBooksPacked), total_bits");
        ret
    }
}

impl Debug for StaticCodeBooks {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("StaticCodeBooks")
        .field("books", &self.books)
        .field("bits_of_books", &format_args!("[{}]", format_array!(self.bits_of_books, ", ", "0x{:04x}")))
        .field("total_bits", &self.total_bits)
        .finish()
    }
}

derive_index!(StaticCodeBooks, StaticCodeBook, books);
