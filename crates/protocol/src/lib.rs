//! Wire protocol shared by the Havana Rust server and its tooling.
//!
//! The plain transport is in [`frame`]; the RC4-like layer that Shockwave clients negotiate with
//! `GENERATEKEY` is in [`crypto`].

pub mod body;
pub mod crypto;
pub mod encoding;
pub mod escape;
pub mod frame;
pub mod ids;

pub use frame::{Decoder, Message};
