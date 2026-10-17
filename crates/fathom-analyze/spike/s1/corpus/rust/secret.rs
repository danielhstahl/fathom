pub fn helper() -> u32 {
    7
}

pub(crate) fn only_inside() -> u32 {
    3
}

pub mod nested_private {
    pub fn deep() -> u32 {
        1
    }
}
