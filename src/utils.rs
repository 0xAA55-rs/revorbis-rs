use std::{
    io,
    fmt::{self, Debug, Display, Formatter},
};

use crate::*;
use bitwise::*;

/// * Format array in a specific patterns
#[macro_export]
macro_rules! format_array {
    () => {
        "".to_string()
    };
    ($data:expr) => {
        format_array!($data, ", ", "{}")
    };
    ($data:expr, hex2) => {
        format_array!($data, " ", "{:02x}")
    };
    ($data:expr, hex4) => {
        format_array!($data, " ", "{:04x}")
    };
    ($data:expr, hex8) => {
        format_array!($data, " ", "{:08x}")
    };
    ($data:expr, hex2arr) => {
        format_array!($data, ", ", "0x{:02x}")
    };
    ($data:expr, hex4arr) => {
        format_array!($data, ", ", "0x{:04x}")
    };
    ($data:expr, hex8arr) => {
        format_array!($data, ", ", "0x{:08x}")
    };
    ($data:expr, $delims:expr, $($arg:tt)*) => {
        $data.iter().map(|&v|format!($($arg)*, v)).collect::<Vec<_>>().join($delims)
    };
}

pub struct NestVecFormatter<'a, T>
where
    T: Display + Copy {
    vec: &'a Vec<T>,
}

impl<'a, T> NestVecFormatter<'a, T>
where
    T: Display + Copy {
    pub fn new(vec: &'a Vec<T>) -> Self {
        Self {
            vec,
        }
    }

    pub fn new_level1(vec: &'a Vec<Vec<T>>) -> Vec<Self> {
        let mut ret = Vec::with_capacity(vec.len());
        for v in vec.iter() {
            ret.push(Self::new(&v))
        }
        ret
    }

    pub fn new_level2(vec: &'a Vec<Vec<Vec<T>>>) -> Vec<Vec<Self>> {
        let mut ret = Vec::with_capacity(vec.len());
        for v in vec.iter() {
            let mut new_v = Vec::with_capacity(v.len());
            for v in v.iter() {
                new_v.push(Self::new(&v))
            }
            ret.push(new_v)
        }
        ret
    }
}

impl<T> Debug for NestVecFormatter<'_, T>
where
    T: Display + Copy {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list()
        .entries(&[format_args!("[{}]", format_array!(&self.vec))])
        .finish()
    }
}

#[macro_export]
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

#[macro_export]
macro_rules! return_Err {
    ($error:expr) => {
        if PANIC_ON_ERROR {
            panic!("{:?}", $error)
        } else {
            return Err($error)
        }
    }
}

#[macro_export]
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

#[macro_export]
macro_rules! rint {
    ($x:expr) => {
        $x.floor() + 0.5
    };
}

use ogg::{OggPacket, OggPacketType};
use io_utils::CursorVecU8;
use vorbis::*;
use codebook::*;

/// * This function extracts data from some Ogg packets, the packets contains the Vorbis headers.
/// * There are 3 kinds of Vorbis headers, they are the identification header, the metadata header, and the setup header.
#[allow(clippy::type_complexity)]
pub fn get_vorbis_headers_from_ogg_packet_bytes(data: &[u8], stream_id: &mut u32) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), io::Error> {
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
    let mut stream_id = 0u32;
    let (identification_header, comment_header, setup_header) = get_vorbis_headers_from_ogg_packet_bytes(data, &mut stream_id)?;

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
