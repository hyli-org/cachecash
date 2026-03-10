mod metadata {
    pub const ELF: &[u8] = include_bytes!("../elf/elf.dat");
}

pub use metadata::*;
