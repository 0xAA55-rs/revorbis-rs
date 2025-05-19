#![allow(dead_code)]
use std::{
    fmt::{self, Debug, Formatter},
    io::{self, Write}
};
use crate::*;

use io_utils::CursorVecU8;
use codebook::StaticCodeBook;

/// * This is the encoding information for the `VorbisSetupHeader` static codebooks section.
/// * The struct stores packed codebooks together, and stores each books size in bits.
/// * This is for the **test** usage.
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

/// * This is the decoding information for the `VorbisSetupHeader` static codebooks section.
/// * The struct stores each decoded books and its size in bits, and the total bits of the books.
/// * This is for the **test** usage.
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

    /// * Pack to bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> Result<usize, io::Error>
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
        .field("bits_of_books", &format_args!("[{}]", format_array!(self.bits_of_books, hex4arr)))
        .field("total_bits", &self.total_bits)
        .finish()
    }
}

derive_index!(StaticCodeBooks, StaticCodeBook, books);

/// * This function removes the codebooks from the Vorbis setup header. The setup header was extracted from the Ogg stream.
/// * Since Vorbis stores data in bitwise form, all of the data are not aligned in bytes, we have to parse it bit by bit.
/// * After parsing the codebooks, we can sum up the total bits of the codebooks, and then we can replace it with an empty codebook.
/// * At last, use our `BitwiseData` to concatenate these bit-strings without any gaps.
pub fn remove_codebook_from_setup_header(setup_header: &[u8]) -> Result<Vec<u8>, io::Error> {
    // Try to verify if this is the right way to read the codebook
    assert_eq!(&setup_header[0..7], b"\x05vorbis", "Checking the vorbis header that is a `setup_header` or not");

    // Let's find the book, and kill it.
    let codebooks = StaticCodeBooks::load_from_slice(&setup_header[7..]).unwrap();
    let bytes_before_codebook = BitwiseData::from_bytes(&setup_header[0..7]);
    let (_codebook_bits, bits_after_codebook) = BitwiseData::new(&setup_header[7..], (setup_header.len() - 7) * 8).split(codebooks.total_bits);

    // Let's generate the empty codebook.
    let _empty_codebooks = StaticCodeBooks::default().to_packed_codebooks().unwrap().books;

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
pub fn _remove_codebook_from_ogg_stream(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    use ogg::{OggPacket, OggPacketType};
    let mut stream_id = 0u32;
    let (identification_header, comment_header, setup_header) = headers::get_vorbis_headers_from_ogg_packet_bytes(data, &mut stream_id)?;

    // Our target is to kill the codebooks from the `setup_header`
    // If this packet doesn't have any `setup_header`
    // We return.
    if setup_header.is_empty() {
        return_Err!(io::Error::new(io::ErrorKind::InvalidData, "There's no setup header in the given Ogg packets.".to_string()));
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
