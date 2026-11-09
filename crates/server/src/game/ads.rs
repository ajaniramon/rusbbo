//! Java: `AdManager`: the `rooms_ads` rows by room (-1 for the loading ads), read once and again
//! on RCON `refreshads`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use rand::RngExt;
use rusbbo_storage::{RoomAd, Store};

#[derive(Default)]
pub struct AdManager {
    ads: Mutex<Option<Arc<HashMap<i32, Vec<RoomAd>>>>>,
}

fn load(store: &Store) -> HashMap<i32, Vec<RoomAd>> {
    let mut map: HashMap<i32, Vec<RoomAd>> = HashMap::new();
    for ad in store.records::<RoomAd>().unwrap_or_default() {
        map.entry(ad.room_id).or_default().push(ad);
    }
    map
}

impl AdManager {
    fn ads(&self, store: &Store) -> Arc<HashMap<i32, Vec<RoomAd>>> {
        self.ads
            .lock()
            .get_or_insert_with(|| Arc::new(load(store)))
            .clone()
    }

    /// `AdManager.reset`: read the table again.
    pub fn reset(&self, store: &Store) {
        *self.ads.lock() = Some(Arc::new(load(store)));
    }

    /// `getRandomAd`: one of the room's enabled ads that is not a loading ad. `Err` when the room
    /// has ads but none of those (Java's `nextInt(0)` throws).
    pub fn random_ad(&self, store: &Store, room_id: i32) -> Result<Option<RoomAd>, ()> {
        let ads = self.ads(store);
        let Some(list) = ads.get(&room_id) else {
            return Ok(None);
        };
        let list: Vec<&RoomAd> = list
            .iter()
            .filter(|a| a.enabled && !a.is_loading_ad)
            .collect();
        if list.is_empty() {
            return Err(());
        }
        Ok(Some(list[rand::rng().random_range(0..list.len())].clone()))
    }

    /// `getRandomLoadingAd`: one of the enabled loading ads (room -1).
    pub fn random_loading_ad(&self, store: &Store) -> Option<RoomAd> {
        let ads = self.ads(store);
        let list: Vec<&RoomAd> = ads
            .get(&-1)
            .map(|l| l.iter().filter(|a| a.enabled && a.is_loading_ad).collect())
            .unwrap_or_default();
        if list.is_empty() {
            return None;
        }
        Some(list[rand::rng().random_range(0..list.len())].clone())
    }
}

/// The image and click URLs the client loads an ad from (`site.path` with `http`, on the
/// website's `/api/advertisement/` routes).
pub fn urls(site_path: &str, ad: &RoomAd) -> (String, String) {
    let base = site_path.replace("https", "http");
    let url = ad
        .url
        .as_ref()
        .map(|_| format!("{base}/api/advertisement/get_url?ad={}", ad.id))
        .unwrap_or_default();
    (
        format!("{base}/api/advertisement/get_img?ad={}", ad.id),
        url,
    )
}
