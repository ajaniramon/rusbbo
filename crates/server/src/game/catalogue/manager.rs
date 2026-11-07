//! Java: `game/catalogue/CatalogueManager.java`.

use std::collections::HashMap;

use anyhow::Result;
use havana_storage::{Store, User, Writer};
use tracing::error;

use super::{CatalogueItem, CataloguePackage, CataloguePage};
use crate::game::currency;
use crate::game::item::base::{InteractionType, ItemBehaviour, ItemDefinition};
use crate::game::item::{DEFAULT_ROOMDIMMER_CUSTOM_DATA, ItemManager};
use crate::game::player::Player;
use crate::messages::outgoing;
use crate::session::Session;
use crate::settings::Settings;

/// Java: `PlayerRank.COMMUNITY_MANAGER`.
pub const COMMUNITY_MANAGER_RANK: i32 = 6;

pub struct CatalogueManager {
    pages: Vec<CataloguePage>,
    items: Vec<CatalogueItem>,
    /// Sale code -> badge codes (Java: `badgeRewards`).
    #[allow(dead_code)] // TODO(badges)
    badge_rewards: HashMap<String, Vec<String>>,
}

impl CatalogueManager {
    /// Java: the `CatalogueManager` constructor (`CatalogueDao.getPages/getPackages/getItems`).
    pub fn load(store: &Store, items: &ItemManager) -> Result<Self> {
        Ok(Self::new(
            store.catalogue_pages()?,
            store.catalogue_items()?,
            store.catalogue_packages()?,
            store.catalogue_sale_badges()?,
            items,
        ))
    }

    pub fn new(
        pages: Vec<havana_storage::CataloguePage>,
        items: Vec<havana_storage::CatalogueItem>,
        packages: Vec<havana_storage::CataloguePackage>,
        badges: Vec<havana_storage::CatalogueSaleBadge>,
        item_manager: &ItemManager,
    ) -> Self {
        let packages: Vec<CataloguePackage> = packages
            .into_iter()
            .map(|p| {
                let definition_id = p.definition_id.unwrap_or(0);
                CataloguePackage {
                    sale_code: p.sale_code.unwrap_or_default(),
                    definition_id,
                    special_sprite_id: p
                        .special_sprite_id
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    amount: p.amount.unwrap_or(0),
                    definition: item_manager.definition(definition_id),
                }
            })
            .collect();
        let mut items: Vec<CatalogueItem> = items
            .into_iter()
            .filter_map(|i| CatalogueItem::new(i, item_manager))
            .collect();
        // Java: loadPackages.
        for item in items.iter_mut().filter(|i| i.is_package) {
            let sale_code = item.sale_code.clone();
            for package in packages.iter().filter(|p| p.sale_code == sale_code) {
                item.add_package(package.clone());
            }
        }
        let mut badge_rewards: HashMap<String, Vec<String>> = HashMap::new();
        for b in badges {
            badge_rewards
                .entry(b.sale_code)
                .or_default()
                .push(b.badge_code);
        }
        Self {
            pages: pages.into_iter().map(CataloguePage::new).collect(),
            items,
            badge_rewards,
        }
    }

    /// Java: `getChildPages`: the pages under `parent_id` this rank (and club status) may see.
    pub fn child_pages(
        &self,
        parent_id: i32,
        rank: i32,
        has_club: bool,
        settings: &Settings,
    ) -> Vec<&CataloguePage> {
        self.pages
            .iter()
            .filter(|page| {
                page.parent_id == parent_id
                    && page.min_role <= rank
                    && (!page.is_club_only || has_club)
                    && (page.is_valid_seasonal(settings) || rank >= COMMUNITY_MANAGER_RANK)
            })
            .collect()
    }

    /// Java: `getCataloguePage`.
    pub fn page(&self, id: i32) -> Option<&CataloguePage> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn pages(&self) -> &[CataloguePage] {
        &self.pages
    }

    /// Java: `getCatalogueItem(int)`.
    pub fn item(&self, id: i32) -> Option<&CatalogueItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Java: `getCatalogueItem(String saleCode)`: the first item with that sale code.
    pub fn item_by_sale_code(&self, sale_code: &str) -> Option<&CatalogueItem> {
        self.items.iter().find(|i| i.sale_code == sale_code)
    }

    /// Java: `ItemManager.createGift`, inside `w`: a present wrapping `sale_code` for
    /// `owner_id`, in a random `present_gen` sprite. Its custom data is
    /// `catalogue id|from|label|extra data|time`. Returns the new row; it becomes a live item
    /// (with a virtual id) once `w` is committed.
    #[allow(clippy::too_many_arguments)]
    pub fn create_gift_in(
        &self,
        w: &mut Writer,
        items: &ItemManager,
        owner_id: u32,
        received_from: &str,
        sale_code: &str,
        label: &str,
        extra_data: &str,
        now: i64,
    ) -> anyhow::Result<havana_storage::Item> {
        use rand::RngExt;
        let present_id = rand::rng().random_range(0..7);
        let sprite = if present_id > 0 {
            format!("present_gen{present_id}")
        } else {
            "present_gen".to_string()
        };
        let (Some(def), Some(catalogue_item)) = (
            items.definition_by_sprite(&sprite),
            self.item_by_sale_code(sale_code),
        ) else {
            // Java throws a NullPointerException here.
            anyhow::bail!("no {sprite} definition or no catalogue item {sale_code}");
        };
        let custom_data = format!(
            "{}|{}|{}|{}|{}",
            catalogue_item.id,
            received_from,
            label.replace('|', ""),
            extra_data.replace('|', ""),
            now
        );
        let draft = havana_storage::Item::with_defaults(owner_id, def.record.id, custom_data, now);
        Ok(w.insert_item(&draft, now)?)
    }

    /// Java: `getSeasonalItems`: the items on sale today, at their seasonal prices.
    pub fn seasonal_items(&self) -> Vec<CatalogueItem> {
        self.items
            .iter()
            .filter(|i| i.is_active())
            .map(|i| {
                let mut item = i.clone();
                item.price_coins = item.seasonal_coins;
                item.price_pixels = item.seasonal_pixels;
                item
            })
            .collect()
    }

    /// Java: `getCataloguePageItems`. With `raw_items` false the rares page lists today's
    /// seasonal items.
    ///
    /// TODO(collectables): collectable pages list only the active collectable.
    pub fn page_items(
        &self,
        page_id: i32,
        raw_items: bool,
        settings: &Settings,
    ) -> Vec<CatalogueItem> {
        if !raw_items && i64::from(page_id) == settings.int("rare.cycle.page.id") {
            let seasonal = self.seasonal_items();
            if !seasonal.is_empty() {
                return seasonal;
            }
        }
        let mut items: Vec<CatalogueItem> = self
            .items
            .iter()
            .filter(|i| !i.hidden && i.has_page(page_id))
            .cloned()
            .collect();
        items.sort_by_key(|i| i.order_id);
        items
    }

    /// Java: `getBadgeRewards().get(saleCode)`.
    #[allow(dead_code)] // TODO(badges)
    pub fn badge_rewards(&self, sale_code: &str) -> &[String] {
        self.badge_rewards.get(sale_code).map_or(&[], Vec::as_slice)
    }

    /// What this port cannot sell yet. The caller refuses the purchase before charging anything.
    ///
    /// TODO(effects): `purchaseEffect` (users_effects, `AVATAR_EFFECT_ADDED`).
    /// TODO(trophies): `PRIZE_TROPHY` extra data (TrophyExtraData JSON, player lookup).
    pub fn unsupported(item: &CatalogueItem) -> Option<&'static str> {
        for package in item.packages() {
            let Some(def) = &package.definition else {
                continue;
            };
            if def.has_behaviour(ItemBehaviour::Effect) {
                return Some("effects");
            }
            if def.has_behaviour(ItemBehaviour::PrizeTrophy) {
                return Some("trophies");
            }
        }
        None
    }

    /// Java: `purchase(PlayerDetails, CatalogueItem, ...)`, inside `w`: creates the items (with
    /// their pets, teleporter links and film) for `owner`. Nothing changes in memory: once `w`
    /// is committed, [`Purchase::apply`] hands the items over; dropping `w` undoes it all.
    pub fn purchase_in(
        &self,
        w: &mut Writer,
        owner: &User,
        item: &CatalogueItem,
        extra_data: &str,
        now: i64,
    ) -> anyhow::Result<Purchase> {
        let mut bought = Purchase::default();
        if !item.is_package {
            let Some(def) = &item.definition else {
                return Ok(bought);
            };
            for _ in 0..item.amount {
                purchase_one(
                    w,
                    owner,
                    def,
                    Some(extra_data),
                    &item.item_special_id,
                    now,
                    &mut bought,
                )?;
            }
        } else {
            for package in item.packages() {
                let Some(def) = &package.definition else {
                    continue;
                };
                for _ in 0..package.amount {
                    purchase_one(
                        w,
                        owner,
                        def,
                        None,
                        &package.special_sprite_id,
                        now,
                        &mut bought,
                    )?;
                }
            }
        }
        Ok(bought)
    }
}

/// One item a purchase created, with the partner of a teleporter.
pub struct BoughtUnit {
    pub item: havana_storage::Item,
    pub linked: Option<havana_storage::Item>,
}

/// What [`CatalogueManager::purchase_in`] wrote, to apply once it is committed.
#[derive(Default)]
pub struct Purchase {
    /// Java's `itemsBought`, in order.
    pub units: Vec<BoughtUnit>,
    /// The stored film after each grant (film itself, a camera's two), one `FILM` each.
    pub film: Vec<i32>,
}

impl Purchase {
    /// Whether no item was created (film is not an item).
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// The ids of the bought items, comma-separated, as a transaction row lists them (Java:
    /// `getDatabaseId` of each of `itemsBought`).
    pub fn id_list(&self) -> String {
        self.units
            .iter()
            .map(|u| u.item.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// After the commit: sends `FILM` for each grant and puts the items in the hand (a
    /// teleporter's partner first, the order of Java's `addItem` calls). Returns the database
    /// ids of the bought items.
    pub fn apply(self, items: &ItemManager, session: &Session, player: &mut Player) -> Vec<u64> {
        if let Some(details) = player.details.as_mut() {
            for film in &self.film {
                details.film = *film;
                session.send(outgoing::user::film(*film));
            }
        }
        let mut ids = Vec::new();
        for unit in self.units {
            let id = unit.item.id;
            let Some(item) = items.make_item(unit.item) else {
                error!(item = id, "bought an item without a definition");
                continue;
            };
            if let Some(linked) = unit.linked.and_then(|l| items.make_item(l)) {
                player.inventory.add_item(linked);
            }
            player.inventory.add_item(item);
            ids.push(id);
        }
        ids
    }
}

/// Java: `GRPC.getTransactionDescription`: the description of a purchase's transaction row, or
/// `None` (no row) for an item without a definition.
pub fn transaction_description(item: &CatalogueItem) -> Option<String> {
    fn describe(def: Option<&ItemDefinition>) -> Option<String> {
        let def = def?;
        Some(if def.has_behaviour(ItemBehaviour::Effect) {
            format!(
                "Effect {} purchase",
                def.sprite().replace("avatar_effect", "")
            )
        } else if def.sprite() == "film" {
            "Film purchase".to_string()
        } else {
            format!("{} purchase", def.record.name)
        })
    }
    if !item.is_package {
        return describe(item.definition.as_deref());
    }
    let parts: Vec<String> = item
        .packages()
        .iter()
        .filter_map(|p| describe(p.definition.as_deref()))
        .collect();
    Some(format!("Package purchase ({})", parts.join(", ")))
}

/// Java: `purchase(PlayerDetails, ItemDefinition, ...)`: creates one item for `owner` inside
/// `w` and records it in `bought`.
fn purchase_one(
    w: &mut Writer,
    owner: &User,
    def: &std::sync::Arc<ItemDefinition>,
    extra_data: Option<&str>,
    special_sprite_id: &str,
    now: i64,
    bought: &mut Purchase,
) -> anyhow::Result<()> {
    // Film is not an item: the player just gets film.
    if def.sprite() == "film" {
        bought
            .film
            .push(currency::increase_film_in(w, owner.id, 5)?);
        return Ok(());
    }

    let mut custom_data = String::new();
    if let Some(extra) = extra_data {
        if def.has_behaviour(ItemBehaviour::Decoration) {
            custom_data = extra.to_string();
        } else if !special_sprite_id.is_empty() {
            custom_data = special_sprite_id.to_string();
        }
        if def.has_behaviour(ItemBehaviour::PostIt) {
            custom_data = "20".into();
        }
        if def.interaction_type == InteractionType::Scoreboard {
            custom_data = "x".into();
        }
        // TODO(trophies): PRIZE_TROPHY builds TrophyExtraData JSON here.
        if def.has_behaviour(ItemBehaviour::Roomdimmer) {
            custom_data = DEFAULT_ROOMDIMMER_CUSTOM_DATA.into();
        }
    }
    if custom_data.is_empty() {
        custom_data = "0".into();
    }
    reset_extra_data(def, &mut custom_data);

    // Java: a pet nest comes with its pet, from `name\nrace\ncolour`; anything invalid (or no
    // extra data) buys nothing. The name is checked against the owner's: the buyer's, or for a
    // present the opener's.
    let pet = if def.interaction_type == InteractionType::PetNest {
        match pet_from_extra_data(&owner.username, def.sprite(), extra_data, now) {
            Some(pet) => Some(pet),
            None => return Ok(()),
        }
    } else {
        None
    };

    let mut draft =
        havana_storage::Item::with_defaults(owner.id, def.record.id, custom_data.clone(), now);
    if def.record.rental_time > 0 {
        draft.expire_time = now + i64::from(def.record.rental_time);
    }

    // A camera comes with two films.
    if def.sprite() == "camera" {
        bought
            .film
            .push(currency::increase_film_in(w, owner.id, 2)?);
    }

    let created = w.insert_item(&draft, now)?;
    if let Some(mut pet) = pet {
        pet.item_id = created.id;
        w.insert_pet(pet)?;
    }
    let linked = if def.has_behaviour(ItemBehaviour::Teleporter) {
        // A teleporter is sold as a pair. The linked one keeps the unreset custom data.
        let mut linked_draft =
            havana_storage::Item::with_defaults(owner.id, def.record.id, custom_data, now);
        if def.record.rental_time > 0 {
            linked_draft.expire_time = now + i64::from(def.record.rental_time);
        }
        let linked = w.insert_item(&linked_draft, now)?;
        // Java: TeleporterDao.addPair both ways.
        w.add_teleporter_link(linked.id, created.id)?;
        w.add_teleporter_link(created.id, linked.id)?;
        Some(linked)
    } else {
        None
    };
    bought.units.push(BoughtUnit {
        item: created,
        linked,
    });
    Ok(())
}

/// The custom data part of Java's `RoomMapping.resetExtraData`: the value a freshly created item
/// starts with. Rules are tried in Java's order; the first one whose value differs from the
/// current one rewrites it and ends the check.
fn reset_extra_data(def: &ItemDefinition, custom_data: &mut String) {
    use InteractionType::*;
    let rules: [(bool, &str); 8] = [
        (def.interaction_type == Lert, "0"),
        (def.interaction_type == IdolVoteChair, "0"),
        (def.interaction_type == IdolScoreboard, "-1"),
        (def.has_behaviour(ItemBehaviour::OneWayGate), "0"),
        (def.interaction_type == Fortune, "8"), // FortuneInteractor.FORTUNE_OFF
        (def.interaction_type == VendingMachine, "0"),
        (def.interaction_type == LoveRandomizer, "0"),
        (def.has_behaviour(ItemBehaviour::Teleporter), "0"), // TELEPORTER_CLOSE
    ];
    for (applies, value) in rules {
        if applies && custom_data != value {
            *custom_data = value.to_string();
            return;
        }
    }
}

/// Java: the `PET_NEST` part of `CatalogueManager.purchase`: `name\nrace\ncolour`, with a name
/// `PetManager.isValidName` accepts, a numeric race and a hex colour; the type is the sprite
/// without `pets` (`pets0` is a dog). The natures are random, as in `PetDao.createPet`.
fn pet_from_extra_data(
    owner: &str,
    sprite: &str,
    extra_data: Option<&str>,
    now: i64,
) -> Option<havana_storage::Pet> {
    use rand::RngExt;
    let extra = extra_data?;
    if !extra.contains('\n') {
        return None;
    }
    let parts: Vec<&str> = extra.split('\n').collect();
    if parts.len() != 3 || !super::super::room::items::is_numeric(parts[1]) {
        return None;
    }
    let name = crate::game::messenger::filter_input(parts[0], true);
    let race: i32 = parts[1].parse().ok()?;
    let colour = crate::game::messenger::filter_input(parts[2], true);
    let hex = |s: &str| (s.len() == 6 || s.len() == 3) && s.chars().all(|c| c.is_ascii_hexdigit());
    if !hex(&colour) || crate::game::pet::name_status(owner, &name) >= 1 {
        return None;
    }
    let mut rng = rand::rng();
    Some(havana_storage::Pet {
        name,
        pet_type: sprite.replace("pets", ""),
        race,
        colour,
        nature_positive: rng.random_range(0..7),
        nature_negative: rng.random_range(0..7),
        born: now,
        last_kip: now,
        last_eat: now,
        last_drink: now,
        last_playtoy: now,
        last_playuser: now,
        ..Default::default()
    })
}
