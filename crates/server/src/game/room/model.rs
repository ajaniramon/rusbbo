//! Room models: door and heightmap (Java: `game/room/models/RoomModel`, `RoomModelManager`).

use crate::game::pathfinder::Position;

/// A parsed `rooms_models` row (Java: `RoomModel`).
#[derive(Debug)]
pub struct RoomModel {
    /// `model_id`, what `rooms.model` refers to.
    pub id: String,
    /// `model_name`, what the client is told and what the special cases test.
    pub name: String,
    pub door_x: i32,
    pub door_y: i32,
    pub door_z: f64,
    pub door_rotation: i32,
    pub size_x: i32,
    pub size_y: i32,
    /// Rows joined with `\r`, each followed by one (what `HEIGHTMAP` sends).
    pub heightmap: String,
    /// `trigger_class`. TODO(triggers): the model triggers (lido, diving deck, game lobbies...).
    #[allow(dead_code)]
    pub trigger: String,
    /// Indexed `x * size_y + y`; `None` is a closed tile.
    heights: Vec<Option<f64>>,
}

impl RoomModel {
    /// Java: `RoomModel` constructor and `parse`.
    pub fn parse(row: &havana_storage::RoomModel) -> Self {
        let lines: Vec<&str> = row.heightmap.split('|').collect();
        let size_y = lines.len() as i32;
        let size_x = lines.first().map_or(0, |l| l.chars().count()) as i32;
        let mut heights = vec![None; (size_x * size_y) as usize];
        let mut heightmap = String::new();
        for (y, line) in lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            for x in 0..size_x as usize {
                // Java indexes every row by the first row's width and would throw on a short row;
                // a missing tile is closed here instead.
                let c = chars.get(x).copied().unwrap_or('x');
                let idx = x * size_y as usize + y;
                // Java: StringUtil.isNumeric (Character.isDigit) on the one-character tile.
                heights[idx] = c.to_digit(10).map(f64::from);
                if x as i32 == row.door_x && y as i32 == row.door_y {
                    heights[idx] = Some(row.door_z);
                }
                heightmap.push(c);
            }
            heightmap.push('\r');
        }
        Self {
            id: row.model_id.clone(),
            name: row.model_name.clone(),
            door_x: row.door_x,
            door_y: row.door_y,
            door_z: row.door_z,
            door_rotation: row.door_dir,
            size_x,
            size_y,
            heightmap,
            trigger: row.trigger_class.clone(),
            heights,
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.size_x && y < self.size_y)
            .then(|| (x * self.size_y + y) as usize)
    }

    /// Java: `getTileState(x, y) == OPEN`.
    pub fn is_open(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.heights[i].is_some())
    }

    /// Java: `getTileHeight`: 0 for closed or outside tiles.
    pub fn tile_height(&self, x: i32, y: i32) -> f64 {
        self.index(x, y)
            .and_then(|i| self.heights[i])
            .unwrap_or(0.0)
    }

    /// Java: `getDoorLocation`.
    pub fn door_location(&self) -> Position {
        Position {
            x: self.door_x,
            y: self.door_y,
            z: self.door_z,
            head_rotation: self.door_rotation,
            body_rotation: self.door_rotation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_heightmap_and_door() {
        let model = RoomModel::parse(&havana_storage::RoomModel {
            model_id: "m".into(),
            model_name: "m".into(),
            door_x: 0,
            door_y: 1,
            door_z: 2.0,
            door_dir: 2,
            heightmap: "x01|x23".into(),
            ..Default::default()
        });
        assert_eq!((model.size_x, model.size_y), (3, 2));
        assert_eq!(model.heightmap, "x01\rx23\r");
        assert!(!model.is_open(0, 0));
        assert!(model.is_open(0, 1)); // the door is always open
        assert_eq!(model.tile_height(0, 1), 2.0);
        assert_eq!(model.tile_height(2, 1), 3.0);
        assert!(!model.is_open(3, 0));
    }
}
