//! What the Flash navigator shows of a room (Java's `FLASH_*` navigator composers read
//! `RoomData` and the room's category for each one).

use super::RoomEntry;
use crate::world::World;

/// A room for the Flash composers.
#[derive(Clone, Debug)]
pub struct FlashRoom {
    pub entry: RoomEntry,
    /// Java: `RoomData.getTags` (`TagDao.getRoomTags`, in table order).
    pub tags: Vec<String>,
    pub icon_data: String,
    /// Java: `room.getCategory().hasAllowTrading()`; `None` when the category is gone (Java
    /// throws while composing, and the message is not sent).
    pub allow_trading: Option<bool>,
}

/// The Flash view of navigator rooms.
pub fn flash_rooms(world: &World, entries: Vec<RoomEntry>) -> Vec<FlashRoom> {
    let categories = world.room_categories();
    let tags = world
        .store
        .records::<rusbbo_storage::Tag>()
        .unwrap_or_default();
    entries
        .into_iter()
        .map(|entry| {
            let icon_data = world
                .loaded_room(entry.id)
                .map(|r| r.lock().data.icon_data.clone())
                .or_else(|| {
                    world
                        .store
                        .room(entry.id)
                        .ok()
                        .flatten()
                        .map(|r| r.icon_data)
                })
                .unwrap_or_default();
            FlashRoom {
                tags: tags
                    .iter()
                    .filter(|t| t.room_id == entry.id)
                    .map(|t| t.tag.clone())
                    .collect(),
                icon_data,
                allow_trading: u32::try_from(entry.category)
                    .ok()
                    .and_then(|c| categories.get(&c))
                    .map(|c| c.allow_trading),
                entry,
            }
        })
        .collect()
}

/// Java: `StringUtil.isNumeric` (digits only, not empty).
fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Java's `String.split(regex)` for a one-character separator: trailing empty strings go.
fn java_split(s: &str, sep: char) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// The integers the Flash composers write for a room's icon (`background|top layer|items`),
/// with Java's exceptions: anything missing in the first part writes `0, 0, 0`; with
/// `item_try` (a `try` around each item) a broken item is left out, else it stops there and
/// `0, 0, 0` follows what was written.
pub fn icon_ints(icon_data: &str, item_try: bool) -> Vec<i32> {
    let parts = java_split(icon_data, '|');
    let number = |s: &str| -> Option<i32> {
        if is_numeric(s) {
            s.parse().ok()
        } else {
            Some(0)
        }
    };
    let head = (|| -> Option<(i32, i32, Vec<&str>)> {
        let background = number(parts.first()?)?;
        let top = number(parts.get(1)?)?;
        let items = java_split(parts.get(2)?, ' ');
        Some((background, top, items))
    })();
    let Some((background, top, items)) = head else {
        return vec![0, 0, 0];
    };
    let mut out = vec![background, top, items.len() as i32];
    for data in items {
        let icon = java_split(data, ',');
        let item = (|| -> Option<(i32, i32)> {
            let position = number(icon.first().copied().unwrap_or(""))?;
            let id = number(icon.get(1)?)?;
            Some((position, id))
        })();
        match item {
            Some((position, id)) => {
                out.push(position);
                out.push(id);
            }
            None if item_try => {}
            None => {
                out.extend([0, 0, 0]);
                return out;
            }
        }
    }
    out
}

/// Java: `Room.getFollowRedirect`: the room whose walkway leads into this one (0 for none).
pub fn follow_redirect(world: &World, room_id: u32) -> u32 {
    world
        .walkways
        .iter()
        .find(|w| w.target_room_id == room_id)
        .map_or(0, |w| w.room_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_data_like_java() {
        assert_eq!(icon_ints("0|0|", true), [0, 0, 0]);
        assert_eq!(icon_ints("", true), [0, 0, 0]);
        assert_eq!(icon_ints("3|2|1,4 2,5", true), [3, 2, 2, 1, 4, 2, 5]);
        assert_eq!(icon_ints("3|2|1,4 9", true), [3, 2, 2, 1, 4]);
        assert_eq!(icon_ints("3|2|1,4 9", false), [3, 2, 2, 1, 4, 0, 0, 0]);
        assert_eq!(icon_ints("x|2|1,4", true), [0, 2, 1, 1, 4]);
    }
}
