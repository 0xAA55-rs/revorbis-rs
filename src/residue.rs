#![allow(dead_code)]
use std::{
    fmt::{self, Debug, Formatter},
    io::{self, Write},
};

use crate::*;
use vorbis::VorbisSetupHeader;
use copiablebuf::CopiableBuffer;

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

    pub classmetric1: [i32; 64],
    pub classmetric2: [i32; 64],
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
            classmetric1: [0; 64],
            classmetric2: [0; 64],
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
            classmetric1: [0; 64],
            classmetric2: [0; 64],
        }
    }
}
