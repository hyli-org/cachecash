#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::match_bool)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::explicit_deref_methods)]
#![allow(clippy::doc_markdown)]
#![deny(missing_docs)]

//! A set of core primitives for use with polybase's zk circuits

mod address;
mod agg_agg;
mod agg_utxo;
mod burn;
mod hyli_utxo;
mod input_note;
mod merkle_path;
mod migrate;
mod note;
mod note_url;
mod points;
mod signature;
mod traits;
mod util;
mod utxo;

pub use address::*;
pub use agg_agg::*;
pub use agg_utxo::*;
pub use burn::*;
pub use hyli_utxo::*;
pub use input_note::*;
pub use merkle_path::*;
pub use migrate::*;
pub use note::*;
pub use note_url::*;
pub use points::*;
pub use signature::*;
pub use traits::*;
pub use util::*;
pub use utxo::*;
