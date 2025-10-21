use base64::Engine;
use serde::{Deserialize, Deserializer, Serializer};

// Custom serializer for Vec<u8> to base64 string
pub fn serialize_base64<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let base64_string = base64::engine::general_purpose::STANDARD.encode(value);
    serializer.serialize_str(&base64_string)
}

pub fn serialize_base64_opt<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(v) => serialize_base64(v, serializer),
        None => serializer.serialize_none(),
    }
}

// Custom deserializer for base64 string to Vec<u8>
pub fn deserialize_base64<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(serde::de::Error::custom)
}

pub fn serialize_hex_0x_prefixed<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", hex::encode(value));
    serializer.serialize_str(&hex_string)
}

pub fn deserialize_hex_0x_prefixed<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    hex::decode(s).map_err(serde::de::Error::custom)
}
