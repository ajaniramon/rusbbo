//! Walkways: tiles of a public room that take you into a connected one (Java:
//! `game/room/handlers/walkways/WalkwaysManager`, `WalkwaysEntrance`).

use tracing::warn;

use crate::game::pathfinder::Position;

/// Java: `WalkwaysEntrance`.
#[derive(Clone, Debug)]
pub struct WalkwayEntrance {
    pub room_id: u32,
    pub target_room_id: u32,
    pub from_coords: Vec<Position>,
    /// Where the walker appears in the target room; `None` is the target's door.
    pub destination: Option<Position>,
}

/// Java: `WalkwaysManager.createWalkway`; rows Java would fail to parse are skipped.
pub fn parse(rows: &[havana_storage::Walkway]) -> Vec<WalkwayEntrance> {
    let mut out = Vec::new();
    for row in rows {
        match parse_one(row) {
            Some(w) => out.push(w),
            None => warn!(?row, "skipping unparsable walkway"),
        }
    }
    out
}

fn parse_one(row: &havana_storage::Walkway) -> Option<WalkwayEntrance> {
    let mut from_coords = Vec::new();
    // Java's `split(" ")` drops trailing empty strings, so "5,25 " is one tile there.
    for coord in row.coords_map.as_deref()?.trim_end_matches(' ').split(' ') {
        let (x, y) = coord.split_once(',')?;
        let y = y.split(',').next()?;
        from_coords.push(Position::new(x.parse().ok()?, y.parse().ok()?));
    }
    let destination = match row.door_position.as_deref() {
        None => None,
        Some(d) => {
            let parts: Vec<i32> = d
                .split(',')
                .map(|p| p.parse().ok())
                .collect::<Option<_>>()?;
            let (x, y, z, rotation) = (
                *parts.first()?,
                *parts.get(1)?,
                *parts.get(2)?,
                *parts.get(3)?,
            );
            let mut p = Position::new(x, y);
            p.z = f64::from(z);
            p.set_rotation(rotation);
            Some(p)
        }
    };
    Some(WalkwayEntrance {
        room_id: u32::try_from(row.room_id).ok()?,
        target_room_id: u32::try_from(row.to_id).ok()?,
        from_coords,
        destination,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows() {
        let rows = [
            havana_storage::Walkway {
                room_id: 45,
                to_id: 69,
                coords_map: Some("20,23 20,24".into()),
                door_position: Some("3,23,0,2".into()),
            },
            havana_storage::Walkway {
                room_id: 34,
                to_id: 35,
                coords_map: Some("28,4".into()),
                door_position: None,
            },
        ];
        let w = parse(&rows);
        assert_eq!(w.len(), 2);
        assert_eq!(
            w[0].from_coords,
            [Position::new(20, 23), Position::new(20, 24)]
        );
        assert_eq!(w[0].destination.unwrap().body_rotation, 2);
        assert!(w[1].destination.is_none());
    }

    #[test]
    fn trailing_space_like_java_split() {
        let rows = [havana_storage::Walkway {
            room_id: 22,
            to_id: 21,
            coords_map: Some("5,25 ".into()),
            door_position: Some("15,1,4,4".into()),
        }];
        let w = parse(&rows);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].from_coords, [Position::new(5, 25)]);
    }
}
