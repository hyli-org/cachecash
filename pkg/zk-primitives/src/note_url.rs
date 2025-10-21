use element::Element;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use web3::signing::keccak256;

use crate::{
    InputNote, Note, bridged_polygon_usdc_note_kind, get_address_for_private_key,
    hash_private_key_for_psi,
};

/// NoteURLPayload is a struct that contains the data required to create a note URL
///
/// These are used to send payments to a user e.g. `https://payy.link/s#<NoteURLPayload>`
#[derive(Debug, Serialize, Deserialize)]
pub struct NoteURLPayload {
    /// The version of the note URL payload
    /// 0 -> old rollup note
    /// 1 -> old rollup note (derived psi)
    /// 2 -> new rollup (derived psi)
    pub version: u8,
    /// The private key of the note
    pub private_key: Element,
    /// The psi of the note, this is only kept for older note urls
    /// which included the psi
    pub psi: Option<Element>,
    /// The value of the note
    pub value: Element,
    /// The referral code of the note
    pub referral_code: String,
}

impl From<&NoteURLPayload> for InputNote {
    fn from(value: &NoteURLPayload) -> Self {
        let psi = value
            .psi
            .unwrap_or_else(|| hash_private_key_for_psi(value.private_key));
        InputNote {
            secret_key: value.private_key,
            note: Note {
                kind: Element::new(2),
                contract: bridged_polygon_usdc_note_kind(),
                address: get_address_for_private_key(value.private_key),
                psi,
                value: value.value,
            },
        }
    }
}

impl From<&InputNote> for NoteURLPayload {
    fn from(input_note: &InputNote) -> Self {
        Self {
            // New notes use the new rollup version
            version: 2,
            private_key: input_note.secret_key,
            psi: None,
            value: input_note.note.value,
            referral_code: String::new(),
        }
    }
}

impl NoteURLPayload {
    /// Gets the commitment for the note represented by the URL payload
    #[must_use]
    pub fn address(&self) -> Element {
        InputNote::from(self).note.address
    }

    /// Gets the commitment for the note represented by the URL payload
    #[must_use]
    pub fn commitment(&self) -> Element {
        InputNote::from(self).note.commitment()
    }

    /// Gets the explicit or derived psi for the note url
    #[must_use]
    pub fn psi(&self) -> Element {
        match self.version {
            0 => self.psi.expect("version 1 should have explicit psi"),
            1 => {
                Element::from_str(&hex::encode(keccak256(&self.private_key.to_be_bytes()))).unwrap()
            }
            2 => hash_private_key_for_psi(self.private_key),
            _ => unreachable!("only version 1, 2 or 3 is supported"),
        }
    }

    /// Encode a note URL payload to a base58-encoded string
    #[must_use]
    pub fn encode_activity_url_payload(&self) -> String {
        let mut bytes = Vec::new();

        // Encode version
        bytes.push(self.version);

        // Encode private_key
        bytes.extend_from_slice(&self.private_key.to_be_bytes());

        // Encode psi if version is 0
        if let Some(psi) = &self.psi {
            if self.version == 0 {
                bytes.extend_from_slice(&psi.to_be_bytes());
            }
        }

        // Encode value with leading zeros
        let value_bytes = self.value.to_be_bytes();
        let leading_zeros = value_bytes.iter().take_while(|&&b| b == 0).count();
        #[allow(clippy::cast_possible_truncation)]
        bytes.push(leading_zeros as u8);
        bytes.extend_from_slice(&value_bytes[leading_zeros..]);

        // Encode referral_code as UTF-8
        bytes.extend_from_slice(self.referral_code.as_bytes());

        // Return Base58-encoded string
        bs58::encode(bytes).into_string()
    }
}

/// Decode a note URL payload from a base58-encoded string
#[must_use]
pub fn decode_activity_url_payload(payload: &str) -> NoteURLPayload {
    let payload_bytes = bs58::decode(payload)
        .into_vec()
        .expect("Failed to decode base58 payload");

    let mut rest = &payload_bytes[..];

    let version = rest[0];
    rest = &rest[1..];

    let private_key_bytes: [u8; 32] = rest[..32]
        .try_into()
        .expect("Not enough bytes for private_key");
    let private_key = Element::from_be_bytes(private_key_bytes);
    rest = &rest[32..];

    let psi = match version {
        0 => {
            let psi_bytes: [u8; 32] = rest[..32].try_into().expect("Not enough bytes for psi");
            rest = &rest[32..];
            Some(Element::from_be_bytes(psi_bytes))
        }
        1 => Some(Element::from_str(&hex::encode(keccak256(&private_key_bytes))).unwrap()),
        2 => Some(hash_private_key_for_psi(private_key)),
        _ => unreachable!("only version 1, 2 or 3 is supported"),
    };

    let leading_zeros = rest[0] as usize;
    rest = &rest[1..];

    let value_len = 32 - leading_zeros;
    let value_without_leading_zeros = &rest[..value_len];
    rest = &rest[value_len..];

    let mut value_bytes = [0u8; 32];
    value_bytes[leading_zeros..].copy_from_slice(value_without_leading_zeros);
    let value = Element::from_be_bytes(value_bytes);

    let referral_code = String::from_utf8(rest.to_vec()).expect("Invalid UTF-8 in referral code");

    NoteURLPayload {
        version,
        private_key,
        psi,
        value,
        referral_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InputNote;

    #[test]
    fn test_roundtrip_from_input_note() {
        // Create an InputNote
        let input_note =
            InputNote::new_from_ephemeral_private_key(Element::new(101), Element::new(1));

        // Convert to NoteURLPayload
        let payload: NoteURLPayload = (&input_note).into();

        // Encode
        let encoded = payload.encode_activity_url_payload();

        println!("encoded: {encoded}");

        // Decode
        let decoded = decode_activity_url_payload(&encoded);

        // Convert back to InputNote
        let round_tripped_note: InputNote = (&decoded).into();

        // Verify
        assert_eq!(round_tripped_note.secret_key, input_note.secret_key);
        assert_eq!(round_tripped_note.note.value, input_note.note.value);
        assert_eq!(round_tripped_note.note.address, input_note.note.address);
        assert_eq!(round_tripped_note.note.psi, input_note.note.psi);
    }
}
