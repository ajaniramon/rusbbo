//! Items and item definitions (Java: `org.alexdev.havana.game.item`).

pub mod base;
mod manager;
mod owned;

pub use manager::ItemManager;
pub use owned::{DEFAULT_ROOMDIMMER_CUSTOM_DATA, Item};
