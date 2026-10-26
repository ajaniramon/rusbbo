//! Java: `game/catalogue/CataloguePage.java`.

use crate::settings::Settings;

/// Java: `PlayerRank.ADMINISTRATOR`, the rank given to pages whose `min_role` is not a rank.
const ADMINISTRATOR: i32 = 8;
/// Java: `PlayerRank.getRankForId`: ranks are 0 to 8.
const MAX_RANK: i32 = 8;

#[derive(Clone, Debug)]
pub struct CataloguePage {
    pub id: i32,
    pub parent_id: i32,
    /// Rank id of `min_role` (Java: `getMinRole().getRankId()`).
    pub min_role: i32,
    pub is_navigatable: bool,
    pub is_club_only: bool,
    pub name: String,
    pub icon: i32,
    pub colour: i32,
    pub layout: String,
    images: Vec<String>,
    texts: Vec<String>,
    #[allow(dead_code)] // TODO(seasonal)
    pub seasonal_start: Option<String>,
    #[allow(dead_code)] // TODO(seasonal)
    pub seasonal_length: i32,
}

impl CataloguePage {
    pub fn new(r: havana_storage::CataloguePage) -> Self {
        Self {
            id: r.id as i32,
            parent_id: r.parent_id,
            min_role: if (0..=MAX_RANK).contains(&r.min_role) {
                r.min_role
            } else {
                ADMINISTRATOR
            },
            is_navigatable: r.is_navigatable,
            is_club_only: r.is_club_only,
            name: r.name,
            icon: r.icon,
            colour: r.colour,
            layout: r.layout,
            images: r.images,
            texts: r.texts,
            seasonal_start: r.seasonal_start,
            seasonal_length: r.seasonal_length,
        }
    }

    /// Java: `getImages`. The front page layout fills in its `{inputN}` placeholders from the
    /// `catalogue.frontpage.input.N` settings; input 1 is an image name without extension.
    pub fn images(&self, settings: &Settings) -> Vec<String> {
        if !self.layout.eq_ignore_ascii_case("frontpage3") {
            return self.images.clone();
        }
        self.images
            .iter()
            .map(|image| {
                let mut text = image.clone();
                for i in 1..5 {
                    let value = settings.string(&format!("catalogue.frontpage.input.{i}"));
                    let value = if i == 1 {
                        value.rfind('.').map_or(value, |dot| &value[..dot])
                    } else {
                        value
                    };
                    text = text.replace(&format!("{{input{i}}}"), value);
                }
                text
            })
            .collect()
    }

    /// Java: `getTexts`.
    pub fn texts(&self, settings: &Settings) -> Vec<String> {
        if !self.layout.eq_ignore_ascii_case("frontpage3") {
            return self.texts.clone();
        }
        self.texts
            .iter()
            .map(|text| {
                let mut text = text.clone();
                for i in 1..5 {
                    let value = settings.string(&format!("catalogue.frontpage.input.{i}"));
                    text = text.replace(&format!("{{input{i}}}"), value);
                }
                text
            })
            .collect()
    }

    /// Java: `isValidSeasonal`.
    ///
    /// TODO(seasonal): with `seasonal.items` on, pages with a `seasonal_start` (a `MM-dd` date,
    /// or `EASTER`) are only valid for `seasonal_length` seconds from that date.
    pub fn is_valid_seasonal(&self, _settings: &Settings) -> bool {
        true
    }
}
