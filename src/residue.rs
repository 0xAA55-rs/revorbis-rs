#![allow(dead_code)]
use std::{
    fmt::{self, Debug, Formatter},
    mem,
    io::{self, Write},
    rc::Rc,
    cell::RefCell,
};

use crate::*;
use utils::NestVecFormatter;
use codec::VorbisDspState;
use bitwise::{BitReader, BitWriter};
use headers::VorbisSetupHeader;
use codebook::CodeBook;
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

pub struct VorbisLookResidue {
    info: Rc<VorbisResidue>,
    parts: i32,
    stages: i32,
    fullbooks: Rc<RefCell<Vec<Rc<CodeBook>>>>,
    phrasebook: Rc<CodeBook>,
    partbooks: Vec<Vec<Option<Rc<CodeBook>>>>,
    partvals: i32,
    decodemap: Vec<Vec<i32>>,
    postbits: i32,
    phrasebits: i32,
    frames: i32,
}

impl VorbisResidue {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader) -> io::Result<Self> {
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

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>) -> io::Result<usize>
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

impl VorbisLookResidue {
    pub fn look(residue: Rc<VorbisResidue>, vorbis_dsp_state: &VorbisDspState) -> VorbisLookResidue {
        let codec_setup = &vorbis_dsp_state.vorbis_info.codec_setup;
        let fullbooks = codec_setup.fullbooks.clone();
        let phrasebook = fullbooks.borrow()[residue.groupbook as usize].clone();
        let dim = phrasebook.dim as usize;
        let parts = residue.partitions;
        let mut maxstage = 0;
        let mut acc = 0;
        let mut partbooks: Vec<Vec<Option<Rc<CodeBook>>>> = (0..parts).map(|_|Vec::default()).collect();
        for j in 0..parts as usize {
            let secondstage_j = residue.secondstages[j];
            let stages = ilog!(secondstage_j);
            if stages != 0 {
                if stages > maxstage {
                    maxstage = stages;
                }
                let partbooks_j = &mut partbooks[j];
                *partbooks_j = (0..stages).map(|_|Option::<Rc<CodeBook>>::None).collect();
                for k in 0..stages as usize {
                    let partbooks_j_k = &mut partbooks_j[k];
                    if (secondstage_j & (1 << k)) != 0 {
                        *partbooks_j_k = Some(fullbooks.borrow()[residue.booklist[acc] as usize].clone());
                        acc += 1;
                    }
                }
            }
        }

        let mut partvals = 1;
        for _ in 0..dim {
            partvals *= parts;
        }

        let mut decodemap: Vec<Vec<i32>> = (0..partvals).map(|_|Vec::default()).collect();
        for j in 0..partvals as usize {
            let mut val = j as i32;
            let mut mult = partvals as i32 / parts;
            let decodemap_j = &mut decodemap[j];
            *decodemap_j = vec![0; dim];
            for k in 0..dim {
                let decodemap_j_k = &mut decodemap_j[k];
                let deco = val / mult;
                val -= deco * mult;
                mult /= parts;
                *decodemap_j_k = deco;
            }
        }

        VorbisLookResidue {
            info: residue.clone(),
            parts,
            stages: maxstage,
            fullbooks,
            phrasebook,
            partbooks,
            partvals,
            decodemap,
            postbits: 0,
            phrasebits: 0,
            frames: 0,
        }
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
        .field("classmetric1", &format_args!("[{}]", format_array!(self.classmetric1, ", ", "{}")))
        .field("classmetric2", &format_args!("[{}]", format_array!(self.classmetric2, ", ", "{}")))
        .finish()
    }
}

impl Default for VorbisResidue {
    fn default() -> Self {
        unsafe {mem::MaybeUninit::<Self>::zeroed().assume_init()}
    }
}

impl Debug for VorbisLookResidue {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisLookResidue")
        .field("info", &self.info)
        .field("parts", &self.parts)
        .field("stages", &self.stages)
        .field("fullbooks", &self.fullbooks)
        .field("phrasebook", &self.phrasebook)
        .field("partbooks", &self.partbooks)
        .field("partvals", &self.partvals)
        .field("decodemap", &NestVecFormatter::new_level1(&self.decodemap))
        .field("postbits", &self.postbits)
        .field("phrasebits", &self.phrasebits)
        .field("frames", &self.frames)
        .finish()
    }
}

impl Default for VorbisLookResidue {
    #[allow(invalid_value)]
    fn default() -> Self {
        unsafe {mem::MaybeUninit::<Self>::zeroed().assume_init()}
    }
}