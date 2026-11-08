//! Java: `messages/outgoing/trade`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use super::catalogue::latin1;
use crate::game::item::Item;
use crate::game::item::base::ItemBehaviour;

/// Java: `TRADEOPEN`: each side's user id and whether they have trading enabled.
pub fn trade_open(
    user_id: u32,
    user_can_trade: bool,
    partner_id: u32,
    partner_can_trade: bool,
) -> Message {
    Message::new(
        104,
        ServerBody::new()
            .int(user_id as i32)
            .bool(user_can_trade)
            .int(partner_id as i32)
            .bool(partner_can_trade)
            .build(),
    )
}

/// Java: `TRADE_ALREADY_OPEN`.
pub fn trade_already_open() -> Message {
    Message::new(105, Vec::new())
}

/// Java: `TRADE_ITEMS`: the offers of both sides, the receiver's first.
pub fn trade_items(
    user_id: u32,
    own_items: &[Item],
    partner_id: u32,
    partner_items: &[Item],
) -> Message {
    let mut body = ServerBody::new()
        .int(user_id as i32)
        .int(own_items.len() as i32);
    for item in own_items {
        body = serialise_item(body, item);
    }
    body = body.int(partner_id as i32).int(partner_items.len() as i32);
    for item in partner_items {
        body = serialise_item(body, item);
    }
    Message::new(108, body.build())
}

fn serialise_item(body: ServerBody, item: &Item) -> ServerBody {
    let wall = item.has_behaviour(ItemBehaviour::WallItem);
    let body = body
        .int(item.virtual_id)
        .string(if wall { b"i" } else { b"s" })
        .int(item.virtual_id)
        .int(item.definition.record.sprite_id)
        .int(-1)
        .int(-1)
        .string(&latin1(&item.record.custom_data))
        .int(-1)
        .int(-1)
        .int(-1);
    if wall { body } else { body.int(-1) }
}

/// Java: `TRADEACCEPT`.
pub fn trade_accept(user_id: u32, accepted: bool) -> Message {
    Message::new(
        109,
        ServerBody::new().int(user_id as i32).bool(accepted).build(),
    )
}

/// Java: `TRADE_CLOSE`: the id of the user who closed the window.
pub fn trade_close(user_closed_id: u32) -> Message {
    Message::new(110, ServerBody::new().int(user_closed_id as i32).build())
}

/// Java: `TRADECONFIRM`.
pub fn trade_confirm() -> Message {
    Message::new(111, Vec::new())
}

/// Java: `TRADE_COMPLETED`.
pub fn trade_completed() -> Message {
    Message::new(112, Vec::new())
}

/// Java: `ITEM_NOT_TRADABLE`.
pub fn item_not_tradable() -> Message {
    Message::new(515, Vec::new())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::game::item::base::ItemDefinition;

    /// An item of the hand with this virtual id, sprite and behaviour.
    pub(crate) fn item(
        virtual_id: i32,
        sprite_id: i32,
        behaviour: &str,
        custom_data: &str,
    ) -> Item {
        let definition = ItemDefinition::new(rusbbo_storage::ItemDefinition {
            id: sprite_id as u32,
            sprite: "x".into(),
            sprite_id,
            behaviour: behaviour.into(),
            interactor: "default".into(),
            is_tradable: true,
            ..Default::default()
        })
        .unwrap();
        Item {
            record: rusbbo_storage::Item {
                id: virtual_id as u64,
                custom_data: custom_data.into(),
                ..Default::default()
            },
            definition: Arc::new(definition),
            virtual_id,
        }
    }

    /// Bodies from `traces/trade.json`; tracebot1 is user 1 (`I`), tracebot5 is 5 (`QA`) and
    /// tracebot6 is 6 (`RA`).
    #[test]
    fn composers_match_trace() {
        assert_eq!(trade_open(1, false, 5, true).body, b"IHQAI");
        assert_eq!(trade_open(1, false, 5, true).header, 104);
        assert_eq!(trade_open(5, true, 6, true).body, b"QAIRAI");
        assert_eq!(trade_already_open().header, 105);
        assert!(trade_already_open().body.is_empty());
        assert_eq!(trade_accept(5, true).body, b"QAI");
        assert_eq!(trade_accept(5, false).body, b"QAH");
        assert_eq!(trade_accept(5, true).header, 109);
        assert_eq!(trade_close(1).body, b"I");
        assert_eq!(trade_close(5).body, b"QA");
        assert_eq!(trade_close(5).header, 110);
        assert_eq!(trade_confirm().header, 111);
        assert!(trade_confirm().body.is_empty());
        assert_eq!(trade_completed().header, 112);
        assert!(trade_completed().body.is_empty());
        assert_eq!(item_not_tradable().header, 515);
    }

    /// `TRADE_ITEMS` after e offered the chair, then f the bench (sprite ids 371 and 323).
    #[test]
    fn trade_items_match_trace() {
        let chair = item(1, 371, "can_sit_on_top", "0");
        let bench = item(3, 323, "can_sit_on_top", "0");
        assert_eq!(trade_items(5, &[], 6, &[]).body, b"QAHRAH");
        assert_eq!(
            trade_items(5, std::slice::from_ref(&chair), 6, &[]).body,
            b"QAIIs\x02I[\\AMM0\x02MMMMRAH"
        );
        assert_eq!(
            trade_items(
                6,
                std::slice::from_ref(&bench),
                5,
                std::slice::from_ref(&chair)
            )
            .body,
            b"RAIKs\x02K[PAMM0\x02MMMMQAIIs\x02I[\\AMM0\x02MMMM"
        );
        assert_eq!(trade_items(5, &[chair], 6, &[bench]).header, 108);
    }

    /// Wall items have a letter `i` and one `-1` less.
    #[test]
    fn wall_items_are_shorter() {
        let poster = item(7, 4, "wall_item", "x");
        let expected = ServerBody::new()
            .int(5)
            .int(1)
            .int(7)
            .string(b"i")
            .int(7)
            .int(4)
            .int(-1)
            .int(-1)
            .string(b"x")
            .int(-1)
            .int(-1)
            .int(-1)
            .int(6)
            .int(0)
            .build();
        assert_eq!(trade_items(5, &[poster], 6, &[]).body, expected);
    }
}
