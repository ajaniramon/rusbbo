//! Changing a user's balances (Java: `dao/mysql/CurrencyDao.java`). Each function writes the row
//! and then copies the stored value into the live details, as the Java DAO does.
//!
//! The `*_in` variants write inside a caller's transaction (a purchase charges in the same
//! transaction that creates the items) and return the stored value, for the caller to copy into
//! the details once the transaction is committed.

use anyhow::{Context, Result};
use rusbbo_storage::{Store, User, Writer};
use tracing::error;

/// Java: `CurrencyDao.decreaseCredits` inside `w`: the stored balance after it. Nothing is taken
/// from a balance that is already empty (judged by the live `details`, as in Java). Fails if the
/// user's row is gone, so the whole transaction is dropped.
pub fn decrease_credits_in(w: &mut Writer, details: &User, amount: i32) -> Result<i32> {
    let amount = if details.credits <= 0 { 0 } else { amount };
    let stored = w
        .update_user(details.id, |u| u.credits -= amount)?
        .with_context(|| format!("user {} is gone", details.id))?;
    Ok(stored.credits)
}

/// Java: `CurrencyDao.decreaseCredits` on its own (a staff command, burning a song): one
/// transaction, and the stored balance copied into `details`.
pub fn decrease_credits(store: &rusbbo_storage::Store, details: &mut User, amount: i32) {
    let result = (|| -> Result<i32> {
        let mut w = store.write()?;
        let credits = decrease_credits_in(&mut w, details, amount)?;
        w.commit()?;
        Ok(credits)
    })();
    match result {
        Ok(credits) => details.credits = credits,
        Err(e) => tracing::error!(user = details.id, "decreasing credits: {e}"),
    }
}

/// Java: `CurrencyDao.decreasePixels` inside `w`; see [`decrease_credits_in`].
pub fn decrease_pixels_in(w: &mut Writer, details: &User, amount: i32) -> Result<i32> {
    let amount = if details.pixels <= 0 { 0 } else { amount };
    let stored = w
        .update_user(details.id, |u| u.pixels -= amount)?
        .with_context(|| format!("user {} is gone", details.id))?;
    Ok(stored.pixels)
}

/// Java: `CurrencyDao.increaseFilm` inside `w`: the stored film after it.
pub fn increase_film_in(w: &mut Writer, user_id: u32, amount: i32) -> Result<i32> {
    let stored = w
        .update_user(user_id, |u| u.film += amount)?
        .with_context(|| format!("user {user_id} is gone"))?;
    Ok(stored.film)
}

/// Java: `CurrencyDao.increasePixels`.
pub fn increase_pixels(store: &Store, details: &mut User, amount: i32) {
    match store.update_user(details.id, |u| u.pixels += amount) {
        Ok(Some(stored)) => details.pixels = stored.pixels,
        Ok(None) => {}
        Err(e) => error!(user = details.id, "increasing pixels: {e}"),
    }
}

/// The balances a charge left in the store: `Some` for each currency it took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Charge {
    pub credits: Option<i32>,
    pub pixels: Option<i32>,
}

/// The end of Java's `GRPC` inside `w`: takes the price (each currency only if it costs
/// something). Copy the result into the details with [`Charge::apply`] after the commit.
pub fn charge_in(w: &mut Writer, details: &User, coins: i32, pixels: i32) -> Result<Charge> {
    Ok(Charge {
        credits: (coins > 0)
            .then(|| decrease_credits_in(w, details, coins))
            .transpose()?,
        pixels: (pixels > 0)
            .then(|| decrease_pixels_in(w, details, pixels))
            .transpose()?,
    })
}

impl Charge {
    /// Copies the committed balances into the live details.
    pub fn apply(self, details: &mut User) {
        if let Some(credits) = self.credits {
            details.credits = credits;
        }
        if let Some(pixels) = self.pixels {
            details.pixels = pixels;
        }
    }
}
