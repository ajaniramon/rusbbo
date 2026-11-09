//! Java: `messages/outgoing/catalogue`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use crate::game::catalogue::{CatalogueItem, CatalogueManager, CataloguePage};
use crate::game::item::base::ItemBehaviour;
use crate::settings::Settings;

pub use rusbbo_protocol::body::latin1;

/// Java: `CATALOGUE_PAGES`: the page tree this rank and club status can see.
pub fn catalogue_pages(
    rank: i32,
    has_club: bool,
    catalogue: &CatalogueManager,
    settings: &Settings,
) -> Message {
    let tabs = catalogue.child_pages(-1, rank, has_club, settings);
    let mut body = ServerBody::new()
        .int(0) // Navigatable
        .int(0) // Colour
        .int(0) // Icon
        .int(-1) // Page id
        .string(b"")
        .int(0)
        .int(tabs.len() as i32);
    for tab in &tabs {
        body = index_node(body, tab);
        body = recursive_index_node(body, tab, rank, has_club, catalogue, settings);
    }
    Message::new(126, body.build())
}

fn index_node(body: ServerBody, page: &CataloguePage) -> ServerBody {
    body.bool(true)
        .int(page.colour)
        .int(page.icon)
        .int(page.id)
        .string(&latin1(&page.name))
        .int(0)
}

fn recursive_index_node(
    mut body: ServerBody,
    parent: &CataloguePage,
    rank: i32,
    has_club: bool,
    catalogue: &CatalogueManager,
    settings: &Settings,
) -> ServerBody {
    let children = catalogue.child_pages(parent.id, rank, has_club, settings);
    body = body.int(children.len() as i32);
    for child in &children {
        body = index_node(body, child);
        body = recursive_index_node(body, child, rank, has_club, catalogue, settings);
    }
    body
}

/// Java: `CATALOGUE_PAGE`. A Flash client gets the `frontpage3` layout's last six texts
/// replaced (Java's `subList` throws with fewer: `None`).
pub fn catalogue_page(
    page: &CataloguePage,
    items: &[CatalogueItem],
    settings: &Settings,
    flash: bool,
) -> Option<Message> {
    let images = page.images(settings);
    let mut texts = page.texts(settings);
    if flash && page.layout.eq_ignore_ascii_case("frontpage3") {
        let keep = texts.len().checked_sub(6)?;
        texts.truncate(keep);
        texts.push(
            "1. Always ask permission from the bill payer first.\r\n2. Send HABBO in a UK SMS to \
             78881. You'll get an SMS back with a voucher code and will be charged \u{c2}\u{a3}3 \
             plus your standard UK SMS rate, normally 10p.\r\n3. Enter the code below to redeem \
             35 Credits."
                .into(),
        );
        texts.push("Redeem a Habbo Voucher code here:".into());
        texts.push("#FAF8CC".into());
        texts.push("#FAF8CC".into());
    }
    let mut body = ServerBody::new()
        .int(page.id)
        .string(&latin1(&page.layout))
        .int(images.len() as i32);
    for image in &images {
        body = body.string(&latin1(image));
    }
    body = body.int(texts.len() as i32);
    for text in &texts {
        body = body.string(&latin1(text));
    }
    body = body.int(items.len() as i32);
    for item in items {
        body = body.int(item.id);
        let sprite = item.definition.as_ref().map(|d| d.sprite());
        match sprite {
            Some(s @ ("poster" | "landscape" | "wallpaper" | "floor")) if !item.is_package => {
                body = body.string(&latin1(&format!("{s} {}", item.item_special_id)));
            }
            _ => body = body.string(&latin1(&item.sale_code)),
        }
        body = body.int(item.price_coins).int(item.price_pixels);

        let packages: Vec<_> = item
            .packages()
            .into_iter()
            .filter(|p| p.definition.is_some())
            .collect();
        body = body.int(packages.len() as i32);
        for package in packages {
            let Some(def) = &package.definition else {
                continue;
            };
            let effect = def.has_behaviour(ItemBehaviour::Effect);
            let kind: &[u8] = if effect {
                b"e"
            } else if def.has_behaviour(ItemBehaviour::WallItem) {
                b"i"
            } else {
                b"s"
            };
            body = body.string(kind);
            body = body.int(if effect {
                package.special_sprite_id.parse().unwrap_or(0)
            } else {
                def.record.sprite_id
            });
            body = body
                .string(&latin1(&package.special_sprite_id))
                .int(package.amount)
                .int(def.rental_time_as_minutes());
        }
    }
    Some(Message::new(127, body.build()))
}

/// Java: `NO_CREDITS`.
pub fn no_credits(not_enough_credits: bool, not_enough_pixels: bool) -> Message {
    Message::new(
        68,
        ServerBody::new()
            .bool(not_enough_credits)
            .bool(not_enough_pixels)
            .build(),
    )
}

/// Java: `DELIVER_PRESENT`: what an opened present held.
pub fn deliver_present(
    definition: &crate::game::item::base::ItemDefinition,
    custom_data: &str,
) -> Message {
    let kind: &[u8] = if definition.has_behaviour(ItemBehaviour::WallItem) {
        b"i"
    } else {
        b"s"
    };
    let sprite = if definition.sprite() == "poster" {
        format!("{} {}", definition.sprite(), custom_data)
    } else {
        definition.sprite().to_string()
    };
    Message::new(
        129,
        ServerBody::new()
            .string(kind)
            .int(definition.record.sprite_id)
            .string(&latin1(&sprite))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::item::ItemManager;

    /// traces/flat-present.json: opening the present.
    #[test]
    fn deliver_present_matches_trace() {
        let chair = crate::game::room::items::tests::chair_plasto();
        let msg = deliver_present(&chair, "");
        assert_eq!(
            (msg.header, msg.body.as_slice()),
            (129, &b"s\x02[\\Achair_plasto\x02"[..])
        );
    }

    #[test]
    fn latin1_replaces_what_does_not_fit() {
        // Seen in the frontpage trace: "Ã" fits, the next character does not.
        assert_eq!(latin1("\u{c3}\u{201a}\u{c2}\u{a3}"), b"\xc3?\xc2\xa3");
    }

    fn window_definition() -> rusbbo_storage::ItemDefinition {
        rusbbo_storage::ItemDefinition {
            id: 550,
            sprite: "window_70s_narrow".into(),
            name: "Small 70s Window".into(),
            sprite_id: 1197,
            behaviour: "wall_item,requires_rights_for_interaction".into(),
            interactor: "default".into(),
            is_tradable: true,
            is_recyclable: true,
            rental_time: -1,
            ..Default::default()
        }
    }

    /// The Windows page of `traces/catalogue.json`, cut to one item.
    #[test]
    fn catalogue_page_matches_trace() {
        let items = ItemManager::new(vec![window_definition()]);
        let page = CataloguePage::new(rusbbo_storage::CataloguePage {
            id: 53,
            layout: "default_3x3".into(),
            images: vec!["ctlg_windows_headline1_en".into()],
            texts: vec!["Ooh, new view!".into()],
            ..Default::default()
        });
        let item = CatalogueItem::new(
            rusbbo_storage::CatalogueItem {
                id: 653,
                sale_code: Some("window_70s_narrow".into()),
                page_id: Some("53".into()),
                price_coins: 3,
                amount: 1,
                definition_id: Some(550),
                ..Default::default()
            },
            &items,
        )
        .unwrap();
        let settings = Settings::new(Default::default());
        assert_eq!(
            catalogue_page(&page, &[item], &settings, false).unwrap().body,
            b"QMdefault_3x3\x02Ictlg_windows_headline1_en\x02IOoh, new view!\x02IYcBwindow_70s_narrow\x02KHIi\x02YkD\x02IM"
        );
    }

    #[test]
    fn catalogue_index_lists_visible_pages() {
        let items = ItemManager::new(Vec::new());
        let page = |id, parent_id, min_role, name: &str| rusbbo_storage::CataloguePage {
            id,
            parent_id,
            min_role,
            is_navigatable: true,
            name: name.into(),
            ..Default::default()
        };
        let catalogue = CatalogueManager::new(
            vec![
                page(1, -1, 1, "Frontpage"),
                page(2, 1, 1, "Child"),
                page(3, -1, 5, "Staff"),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &items,
        );
        let settings = Settings::new(Default::default());
        // rank 1 sees Frontpage with its child; Staff (rank 5) is hidden.
        assert_eq!(
            catalogue_pages(1, false, &catalogue, &settings).body,
            b"HHHM\x02HIIHHIFrontpage\x02HIIHHJChild\x02HH"
        );
    }

    #[test]
    fn no_credits_bytes() {
        assert_eq!(no_credits(true, false).body, b"IH");
        assert_eq!(no_credits(true, false).header, 68);
    }
}
