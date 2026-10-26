//! Wire protocol shared by the Havana Rust server and its tooling.
//!
//! Only the unencrypted transport is covered so far; the RC4 layer that Shockwave clients
//! negotiate with `GENERATEKEY` comes later.

pub mod body;
pub mod encoding;
pub mod escape;
pub mod frame;
pub mod ids;

pub use frame::{Decoder, Message};
