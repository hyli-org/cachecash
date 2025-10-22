mod metadata {
    pub const HYLI_UTXO_STATE_ELF: &[u8] = include_bytes!("../elf/hyli-utxo-state");
    pub const HYLI_UTXO_STATE_VK: &[u8] = include_bytes!("../elf/hyli-utxo-state_vk");
}

pub use metadata::*;
