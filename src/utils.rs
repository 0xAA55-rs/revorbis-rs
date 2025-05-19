use std::{
    fmt::{self, Debug, Display, Formatter},
};

use crate::*;

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
    ($data:expr, debug) => {
        format_array!($data, ", ", "{:?}")
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
        use std::ops::{Index, IndexMut, Range, RangeFrom, RangeTo, RangeFull};

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
        ($x.floor() + 0.5) as i32
    };
}

#[macro_export]
macro_rules! vecvec {
    [[$val:expr; $len1:expr]; $len2:expr] => {
        (0..$len2).map(|_|vec![$val; $len1]).collect::<Vec<_>>()
    }
}

#[macro_export]
macro_rules! field {
    ($prev:ident, $self:ident, $field:tt) => {
        $prev.field(stringify!($field), &$self.$field)
    }
}

#[macro_export]
macro_rules! field_array {
    ($prev:ident, $self:ident, $field:tt) => {
        $prev.field(stringify!($field), &format_args!("[{}]", format_array!($self.$field)))
    }
}
