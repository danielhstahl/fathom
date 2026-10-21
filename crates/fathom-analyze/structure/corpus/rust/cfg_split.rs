//! Conditional compilation: two definitions of one name, both in the file, one of them in any
//! given build. `e3.2`'s graph has one node per name.

#[cfg(unix)]
pub mod platform {
    pub fn open(path: &str) -> String {
        format!("unix:{path}")
    }

    pub const SEP: char = '/';
}

#[cfg(not(unix))]
pub mod platform {
    pub fn open(path: &str) -> String {
        format!("win:{path}")
    }

    pub const SEP: char = '\\';
}

#[cfg(test)]
mod test_only {
    pub fn assert_sep() {}
}

pub fn use_sep() -> char {
    platform::SEP
}

#[cfg_attr(feature = "extra", allow(dead_code))]
pub fn attributed(v: u8) -> u8 {
    v
}

#[cfg(feature = "extra")]
#[cfg_attr(docsrs, doc(cfg(feature = "extra")))]
pub mod gated {
    pub fn only_with_extra() {}
}
