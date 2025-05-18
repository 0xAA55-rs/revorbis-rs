#![allow(dead_code)]
use std::{
    cmp::max,
    fmt::{self, Debug, Formatter},
    io::{self, Write},
    ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull},
};

use crate::*;
use io_utils::CursorVecU8;

fn bitreverse(mut x: u32) -> u32 {
    x = ((x >> 16) & 0x0000ffff) | ((x << 16) & 0xffff0000);
    x = ((x >>  8) & 0x00ff00ff) | ((x <<  8) & 0xff00ff00);
    x = ((x >>  4) & 0x0f0f0f0f) | ((x <<  4) & 0xf0f0f0f0);
    x = ((x >>  2) & 0x33333333) | ((x <<  2) & 0xcccccccc);
    x = ((x >>  1) & 0x55555555) | ((x <<  1) & 0xaaaaaaaa);
    x
}

fn make_words(lengthlist: &[i8], n: i32, sparsecount: i32) -> Result<Vec<u32>, io::Error> {
    let mut count = 0usize;
    let n = n as usize;
    let sparsecount = sparsecount as usize;
    let mut marker = [0u32; 33];
    let mut ret = vec![0u32; if sparsecount != 0 {sparsecount} else {n}];

    for i in 0..n {
        let length = lengthlist[i] as usize;
        if length > 0 {
            let mut entry = marker[length];
            /* when we claim a node for an entry, we also claim the nodes
               below it (pruning off the imagined tree that may have dangled
               from it) as well as blocking the use of any nodes directly
               above for leaves */

            /* update ourself */
            if length < 32 && (entry >> length) != 0 {
                /* error condition; the lengths must specify an overpopulated tree */
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("The lengths must specify an overpopulated tree. Length: {length}")));
            }

            ret[count] = entry;
            count += 1;

            /* Look to see if the next shorter marker points to the node
               above. if so, update it and repeat.  */
            for j in (1..length + 1).rev() {
                if marker[j] & 1 != 0 {
                    if j == 1 {
                        marker[1] += 1;
                    } else {
                        marker[j] = marker[j - 1] << 1;
                    }
                    break; /* invariant says next upper marker would already
                              have been moved if it was on the same path */
                }
                marker[j] += 1;
            }

            /* prune the tree; the implicit invariant says all the longer
               markers were dangling from our just-taken node.  Dangle them
               from our *new* node. */
            for j in (length + 1)..33 {
                if marker[j] >> 1 == entry {
                    entry = marker[j];
                    marker[j] = marker[j - 1] << 1;
                } else {
                    break;
                }
            }
        } else {
            if sparsecount == 0 {
                count += 1;
            }
        }
    }
    /* any underpopulated tree must be rejected. */
    /* Single-entry codebooks are a retconned extension to the spec.
       They have a single codeword '0' of length 1 that results in an
       underpopulated tree. Shield that case from the underformed tree check. */
    if !(count == 1 && marker[2] == 2) {
        for i in 1..33 {
            if (marker[i] & (0xffffffff >> (32 - i))) != 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Underpopulated tree. `marker[i]`: {}", marker[i])));
            }
        }
    }

    /* bitreverse the words because our bitwise packer/unpacker is LSb
       endian */
    count = 0;
    for i in 0..n {
        let mut temp = 0u32;
        for j in 0..lengthlist[i] as usize {
            temp <<= 1;
            temp |= (ret[count] >> j) & 1;
        }

        if sparsecount != 0 {
            if lengthlist[i] != 0 {
                ret[count] = temp;
                count += 1;
            }
        } else {
            ret[count] = temp;
            count += 1;
        }
    }

    Ok(ret)
}

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
    pub q_sequencep: bool,
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
                ret.q_sequencep = read_bits!(bitreader, 1) != 0;

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
    /// thought of it. Therefore, we opt on the side of caution
    pub fn book_maptype1_quantvals(&self) -> i32 {
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

    /// * unpack the quantized list of values for encode/decode.
    /// * we need to deal with two map types: in map type 1, the values are
    ///   generated algorithmically (each column of the vector counts through
    ///   the values in the quant vector). in map type 2, all the values came
    ///   in in an explicit list. Both value lists must be unpacked.
    pub fn book_unquantize(&self, n: usize, sparsemap: Option<&[u32]>) -> Result<Vec<f32>, io::Error> {
        let mut ret = vec![0.0; n * self.dim as usize];
        let mut count = 0usize;
        /* maptype 1 and 2 both use a quantized value vector, but
           different sizes */
        match self.maptype {
            1 => {
                let quantvals = self.book_maptype1_quantvals() as usize;
                for j in 0..self.entries as usize {
                    if sparsemap.is_some() && self.lengthlist[j] != 0 || sparsemap.is_none() {
                        let mut last = 0.0;
                        let mut indexdiv = 1;
                        for k in 0..self.dim as usize {
                            let index = (j / indexdiv) % quantvals;
                            let val = (self.quantlist[index] as f32).abs() * self.q_delta + self.q_min + last;
                            if self.q_sequencep {
                                last = val;
                            }
                            if let Some(sparsemap) = sparsemap {
                                ret[sparsemap[count] as usize * self.dim as usize + k] = val;
                            } else {
                                ret[count * self.dim as usize + k] = val;
                            }
                            indexdiv *= quantvals;
                        }
                        count += 1;
                    }
                }
                Ok(ret)
            }
            2 => {
                for j in 0..self.entries as usize {
                    if sparsemap.is_some() && self.lengthlist[j] != 0 || sparsemap.is_none() {
                        let mut last = 0.0;
                        for k in 0..self.dim as usize {
                            let val = (self.quantlist[j * self.dim as usize + k] as f32).abs() * self.q_delta + self.q_min + last;
                            if self.q_sequencep {
                                last = val;
                            }
                            if let Some(sparsemap) = sparsemap {
                                ret[sparsemap[count] as usize * self.dim as usize + k] = val;
                            } else {
                                ret[count * self.dim as usize + k] = val;
                            }
                        }
                        count += 1;
                    }
                }
                Ok(ret)
            }
            o => {
                return_Err!(io::Error::new(io::ErrorKind::InvalidInput, format!("Bad map type: {o}")));
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

/// * This is the codebook for encoding and decoding, it's dynamic, and won't be packed into the Vorbis file.
#[derive(Default, Clone, PartialEq)]
pub struct CodeBook {
    /// codebook dimensions (elements per vector)
    pub dim: i32,

    /// codebook entries
    pub entries: i32,

    /// populated codebook entries
    pub used_entries: i32,

    /// The source book
    pub static_codebook: Option<StaticCodeBook>,

    // for encode, the below are entry-ordered, fully populated
    // for decode, the below are ordered by bitreversed codeword and only used entries are populated

    /// list of dim*entries actual entry values
    pub value_list: Vec<f32>,

    /// list of bitstream codewords for each entry
    pub code_list: Vec<u32>,

    /// only used if sparseness collapsed
    pub dec_index: Vec<i32>,
    pub dec_codelengths: Vec<i8>,

    pub dec_firsttablen: i8,
    pub dec_firsttable: Vec<u32>,
    pub dec_maxlength: i8,

    /// The current encoder uses only centered, integer-only lattice books.
    pub quantvals: i32,
    pub minval: f32,
    pub delta: f32,
}

impl CodeBook {
    pub fn new(for_encode: bool, src: &StaticCodeBook) -> Result<Self, io::Error> {
        if for_encode {
            Self::new_for_encode(src)
        } else {
            Self::new_for_decode(src)
        }
    }

    pub fn new_for_encode(src: &StaticCodeBook) -> Result<Self, io::Error> {
        Ok(Self {
            dim: src.dim,
            entries: src.entries,
            used_entries: src.entries,
            static_codebook: Some(src.clone()),
            code_list: make_words(&src.lengthlist, src.entries, 0)?,
            quantvals: src.book_maptype1_quantvals(),
            minval: src.q_min,
            delta: src.q_delta,
            ..Default::default()
        })
    }

    /// Decode codebook arrangement is more heavily optimized than encode
    pub fn new_for_decode(src: &StaticCodeBook) -> Result<Self, io::Error> {
        /* count actually used entries and find max length */
        let mut n = 0usize;
        let used_entries = src.entries;
        for i in 0..src.entries as usize {
            if src.lengthlist[i] > 0 {
                n += 1;
            }
        }

        if n == 0 {
            Ok(Self {
                dim: src.dim,
                entries: src.entries,
                used_entries,
                ..Default::default()
            })
        } else {
            /* two different remappings go on here.

            First, we collapse the likely sparse codebook down only to
            actually represented values/words.  This collapsing needs to be
            indexed as map-valueless books are used to encode original entry
            positions as integers.

            Second, we reorder all vectors, including the entry index above,
            by sorted bitreversed codeword to allow treeless decode. */

            /* perform sort */
            let mut codes = make_words(&src.lengthlist, src.entries, used_entries)?;
            let mut codes_r = Vec::<&u32>::with_capacity(n);

            for i in 0..n {
                codes[i] = bitreverse(codes[i]);
            }
            for i in 0..n {
                codes_r.push(&codes[i]);
            }

            codes_r.sort_by(|a, b|((a > b) as i32).cmp(&((a < b) as i32)));

            let mut sortindex = Vec::<u32>::with_capacity(n);
            let mut code_list = Vec::<u32>::with_capacity(n);

            // the index is a reverse index
            for i in 0..n {
                let position = unsafe{(codes_r[i] as *const u32).offset_from(codes.as_ptr())} as usize;
                sortindex[position] = i as u32;
            }

            for i in 0..n {
                code_list[sortindex[i] as usize] = codes[i];
            }
            let value_list = src.book_unquantize(n, Some(&sortindex))?;

            let mut dec_index = vec![0; n];
            n = 0;
            for i in 0..src.entries as usize {
                if src.lengthlist[i] > 0 {
                    dec_index[sortindex[n] as usize] = i as i32;
                    n += 1;
                }
            }

            let mut dec_codelengths = vec![0; n];
            n = 0;
            for i in 0..src.entries as usize {
                if src.lengthlist[i] > 0 {
                    dec_codelengths[sortindex[n] as usize] = src.lengthlist[i];
                    n += 1;
                }
            }
            let dec_maxlength: i8 = src.lengthlist.iter().copied().max().unwrap();

            let mut dec_firsttablen;
            let mut dec_firsttable;
            if n == 1 && dec_maxlength == 1 {
                dec_firsttablen = 1;
                dec_firsttable = vec![1, 1];
            } else {
                dec_firsttablen = ilog!(used_entries) - 4; // this is magic
                dec_firsttablen = dec_firsttablen.clamp(5, 8);
                let tabn = 1 << dec_firsttablen;
                dec_firsttable = vec![0; tabn];

                for i in 0..n {
                    if dec_codelengths[i] <= dec_firsttablen {
                        let orig = bitreverse(code_list[i]) as usize;
                        for j in 0..1 << (dec_firsttablen - dec_codelengths[i]) {
                            dec_firsttable[orig | (j << dec_codelengths[i])] = (i + 1) as u32;
                        }
                    }
                }
                /* now fill in 'unused' entries in the firsttable with hi/lo search
                   hints for the non-direct-hits */
                let mask = 0xFFFFFFFEu32 << (31 - dec_firsttablen);
                let mut lo = 0;
                let mut hi = 0;
                for _ in 0..tabn {
                    let word = (1 << (32 - dec_firsttablen)) as u32;
                    if dec_firsttable[bitreverse(word) as usize] == 0 {
                        while lo + 1 < n && code_list[lo + 1] < word {
                            lo += 1;
                        }
                        while hi < n && word >= (code_list[hi] & mask) {
                            hi += 1;
                        }

                        let loval = (lo).clamp(0, 0x7FFF) as u32;
                        let hival = (n - hi).clamp(0, 0x7FFF) as u32;
                        dec_firsttable[bitreverse(word) as usize] = 0x80000000u32 | (loval << 15)  | hival;
                    }
                }
            }

            Ok(Self {
                dim: src.dim,
                entries: src.entries,
                used_entries,
                value_list,
                code_list,
                dec_index,
                dec_codelengths,
                dec_firsttablen,
                dec_firsttable,
                dec_maxlength,
                ..Default::default()
            })
        }
    }
}

impl Debug for CodeBook {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("CodeBook")
        .field("dim", &self.dim)
        .field("entries", &self.entries)
        .field("used_entries", &self.used_entries)
        .field("static_codebook", &self.static_codebook)
        .field("value_list", &format_args!("[{}]", format_array!(self.value_list, ", ", "{}")))
        .field("code_list", &format_args!("[{}]", format_array!(self.code_list, ", ", "{}")))
        .field("dec_index", &format_args!("[{}]", format_array!(self.dec_index, ", ", "{}")))
        .field("dec_codelengths", &format_args!("[{}]", format_array!(self.dec_codelengths, ", ", "{}")))
        .field("dec_firsttablen", &self.dec_firsttablen)
        .field("dec_firsttable", &format_args!("[{}]", format_array!(self.dec_firsttable, ", ", "{}")))
        .field("dec_maxlength", &self.dec_maxlength)
        .field("quantvals", &self.quantvals)
        .field("minval", &self.minval)
        .field("delta", &self.delta)
        .finish()
    }
}
