//! Definitions a macro writes are not in the token stream a parser sees.

#[macro_export]
macro_rules! named {
    ($n:ident) => {
        pub fn $n() -> &'static str {
                            stringify!($n)
        }
    };
}

macro_rules! pair {
    ($a:ident, $b:ident) => {
        pub fn $a() -> u32 {
            1
        }

        pub fn $b() -> u32 {
            2
        }
    };
}

named!(from_macro);
pair!(left, right);

pub fn caller() -> u32 {
    from_macro().len() as u32 + left() + right()
}
