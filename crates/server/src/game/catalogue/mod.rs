//! The catalogue (Java: `org.alexdev.havana.game.catalogue`).

pub mod collectables;
mod item;
mod manager;
mod page;
pub mod voucher;

pub use item::{CatalogueItem, CataloguePackage};
pub use manager::{
    COMMUNITY_MANAGER_RANK, CatalogueManager, Purchase, collectable_page_items,
    transaction_description,
};
pub use page::CataloguePage;
