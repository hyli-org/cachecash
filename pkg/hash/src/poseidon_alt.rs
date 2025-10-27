use acvm::{AcirField, FieldElement};
use element::Element;
use once_cell::sync::Lazy;

use crate::poseidon_alt_data::{
    FULL_ROUNDS, MDS_HEX, PARTIAL_ROUNDS, RATE, ROUND_CONSTANTS_HEX, WIDTH,
};

static ROUND_CONSTANTS: Lazy<[[FieldElement; 3]; 65]> =
    Lazy::new(|| ROUND_CONSTANTS_HEX.map(|row| row.map(field_element_from_hex)));

static MDS_MATRIX: Lazy<[[FieldElement; 3]; 3]> =
    Lazy::new(|| MDS_HEX.map(|row| row.map(field_element_from_hex)));

fn field_element_from_hex(hex: &str) -> FieldElement {
    let bytes = hex_to_32_bytes(hex);
    FieldElement::from_be_bytes_reduce(&bytes)
}

fn hex_to_32_bytes(hex: &str) -> [u8; 32] {
    let mut hex = hex.trim_start_matches("0x").to_owned();
    if hex.len() % 2 == 1 {
        hex.insert(0, '0');
    }
    let mut bytes = [0u8; 32];
    let decoded = hex::decode(&hex).expect("invalid hex literal");
    bytes[32 - decoded.len()..].copy_from_slice(&decoded);
    bytes
}

fn sbox(x: FieldElement) -> FieldElement {
    let x2 = x * x;
    let x4 = x2 * x2;
    x4 * x
}

fn apply_mds(state: [FieldElement; WIDTH]) -> [FieldElement; WIDTH] {
    let mut result = [FieldElement::zero(); WIDTH];
    for (slot, row) in result.iter_mut().zip(MDS_MATRIX.iter()) {
        *slot = row
            .iter()
            .zip(state.iter())
            .fold(FieldElement::zero(), |acc, (coef, value)| {
                acc + (*coef * *value)
            });
    }
    result
}

fn full_round(
    mut state: [FieldElement; WIDTH],
    round_constants: [FieldElement; WIDTH],
) -> [FieldElement; WIDTH] {
    for i in 0..WIDTH {
        state[i] = sbox(state[i] + round_constants[i]);
    }
    apply_mds(state)
}

fn partial_round(
    mut state: [FieldElement; WIDTH],
    round_constants: [FieldElement; WIDTH],
) -> [FieldElement; WIDTH] {
    for i in 0..WIDTH {
        state[i] += round_constants[i];
    }
    state[0] = sbox(state[0]);
    apply_mds(state)
}

fn permute(mut state: [FieldElement; WIDTH]) -> [FieldElement; WIDTH] {
    let mut constants = ROUND_CONSTANTS.iter();

    for _ in 0..(FULL_ROUNDS / 2) {
        let round = *constants.next().expect("round constant missing");
        state = full_round(state, round);
    }

    for _ in 0..PARTIAL_ROUNDS {
        let round = *constants.next().expect("round constant missing");
        state = partial_round(state, round);
    }

    for _ in 0..(FULL_ROUNDS / 2) {
        let round = *constants.next().expect("round constant missing");
        state = full_round(state, round);
    }

    state
}

struct PoseidonSponge {
    state: [FieldElement; WIDTH],
    cache: [FieldElement; RATE],
    cache_size: usize,
}

impl PoseidonSponge {
    fn new(initial_capacity: FieldElement) -> Self {
        let mut state = [FieldElement::zero(); WIDTH];
        state[RATE] = initial_capacity;
        Self {
            state,
            cache: [FieldElement::zero(); RATE],
            cache_size: 0,
        }
    }

    fn absorb(&mut self, input: FieldElement) {
        if self.cache_size == RATE {
            self.perform_duplex();
            self.cache[0] = input;
            self.cache_size = 1;
        } else {
            self.cache[self.cache_size] = input;
            self.cache_size += 1;
        }
    }

    fn perform_duplex(&mut self) {
        for (slot, cached) in self
            .state
            .iter_mut()
            .zip(self.cache.iter())
            .take(self.cache_size)
        {
            *slot += *cached;
        }
        self.state = permute(self.state);
        self.cache_size = 0;
    }

    fn squeeze(mut self) -> FieldElement {
        self.perform_duplex();
        self.state[0]
    }
}

/// Compute the Poseidon hash used by the Noir `poseidon_alt` crate for two elements.
#[must_use]
pub fn poseidon_alt_hash_2(elements: [Element; 2]) -> Element {
    let initial_capacity =
        FieldElement::from(2u128) * FieldElement::from(0x1_0000_0000_0000_0000u128);

    let mut sponge = PoseidonSponge::new(initial_capacity);
    sponge.absorb(elements[0].to_base());
    sponge.absorb(elements[1].to_base());

    let result = sponge.squeeze();

    Element::from_base(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn matches_noir_zero_zero() {
        let expected =
            Element::from_str("0x2ba00861b8f1581f5e17d438e323fa2809f58f1a60009dcd05edb1c9c7c833da")
                .unwrap();
        let result = poseidon_alt_hash_2([Element::ZERO, Element::ZERO]);
        assert_eq!(result, expected);
    }

    #[test]
    fn matches_noir_one_zero() {
        let expected =
            Element::from_str("0x034797ee520c67ec5ee6fd37f581ba7267449a21de06aa55a09821caeec2e89d")
                .unwrap();
        let result = poseidon_alt_hash_2([Element::ONE, Element::ZERO]);
        assert_eq!(result, expected);
    }

    #[test]
    fn matches_noir_owner() {
        let expected =
            Element::from_str("0x16cee71675328aab01b7e6ff986920b333f0fa5a155df015cdef5c8386d5f170")
                .unwrap();
        let owner = Element::from(101u64);
        let result = poseidon_alt_hash_2([owner, Element::ZERO]);
        assert_eq!(result, expected);
    }
}
