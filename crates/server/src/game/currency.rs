//! Changing a user's balances (Java: `dao/mysql/CurrencyDao.java`). Each function writes the row
//! and then copies the stored value into the live details, as the Java DAO does.

use havana_storage::{Store, User};
use tracing::error;

/// Java: `CurrencyDao.decreaseCredits`. Nothing is taken from a balance that is already empty.
pub fn decrease_credits(store: &Store, details: &mut User, amount: i32) {
    let amount = if details.credits <= 0 { 0 } else { amount };
    match store.update_user(details.id, |u| u.credits -= amount) {
        Ok(Some(stored)) => details.credits = stored.credits,
        Ok(None) => {}
        Err(e) => error!(user = details.id, "decreasing credits: {e}"),
    }
}

/// Java: `CurrencyDao.decreasePixels`.
pub fn decrease_pixels(store: &Store, details: &mut User, amount: i32) {
    let amount = if details.pixels <= 0 { 0 } else { amount };
    match store.update_user(details.id, |u| u.pixels -= amount) {
        Ok(Some(stored)) => details.pixels = stored.pixels,
        Ok(None) => {}
        Err(e) => error!(user = details.id, "decreasing pixels: {e}"),
    }
}

/// Java: `CurrencyDao.increaseFilm`.
pub fn increase_film(store: &Store, details: &mut User, amount: i32) {
    match store.update_user(details.id, |u| u.film += amount) {
        Ok(Some(stored)) => details.film = stored.film,
        Ok(None) => {}
        Err(e) => error!(user = details.id, "increasing film: {e}"),
    }
}
