//! A small world for handler tests: two users, a few item definitions and catalogue items, a
//! private room, and clients whose outgoing frames can be read back.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use havana_storage::{
    CatalogueItem, CataloguePage, Item, ItemDefinition, Room, RoomModel, Store, User,
    UserStatistics,
};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::game::inventory::Inventory;
use crate::messages::Malformed;
use crate::session::{Outgoing, Session};
use crate::world::World;

pub const ALICE: u32 = 1;
pub const BOB: u32 = 2;
/// tracebot-like private room owned by Alice.
pub const ROOM: u32 = 1000;

pub const CHAIR: i32 = 100;
pub const TELEPORTER: i32 = 101;
pub const DOG: i32 = 102;
pub const DOG_DEFINITION: u32 = 12;

pub struct Fixture {
    _dir: tempfile::TempDir,
    pub world: World,
}

fn definition(
    id: u32,
    sprite: &str,
    name: &str,
    interactor: &str,
    behaviour: &str,
) -> ItemDefinition {
    ItemDefinition {
        id,
        sprite: sprite.into(),
        name: name.into(),
        interactor: interactor.into(),
        behaviour: behaviour.into(),
        is_tradable: true,
        ..Default::default()
    }
}

fn catalogue_item(id: u32, sale_code: &str, definition_id: i32, price: i32) -> CatalogueItem {
    CatalogueItem {
        id,
        sale_code: Some(sale_code.into()),
        page_id: Some("1".into()),
        price_coins: price,
        amount: 1,
        definition_id: Some(definition_id),
        ..Default::default()
    }
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        for (id, name, credits) in [(ALICE, "alice", 100), (BOB, "bob", 50)] {
            w.put_user(&User {
                id,
                username: name.into(),
                credits,
                pixels: 20,
                rank: 1,
                trade_enabled: true,
                ..Default::default()
            })
            .unwrap();
        }
        for def in [
            definition(10, "chair_plasto", "Chair", "chair", "CAN_SIT_ON_TOP"),
            definition(11, "door", "Teleport Door", "teleport", "TELEPORTER"),
            definition(DOG_DEFINITION, "pets0", "Dog", "pet_nest", ""),
        ] {
            w.put_item_definition(&def).unwrap();
        }
        for (i, sprite) in [
            "present_gen",
            "present_gen1",
            "present_gen2",
            "present_gen3",
        ]
        .into_iter()
        .chain(["present_gen4", "present_gen5", "present_gen6"])
        .enumerate()
        {
            w.put_item_definition(&definition(
                20 + i as u32,
                sprite,
                "Gift",
                "default",
                "PRESENT",
            ))
            .unwrap();
        }
        w.put_catalogue_page(&CataloguePage {
            id: 1,
            min_role: 1,
            is_navigatable: true,
            name: "Shop".into(),
            ..Default::default()
        })
        .unwrap();
        for item in [
            catalogue_item(CHAIR as u32, "chair_plasto", 10, 3),
            catalogue_item(TELEPORTER as u32, "door", 11, 5),
            catalogue_item(DOG as u32, "pets0", DOG_DEFINITION as i32, 4),
        ] {
            w.put_catalogue_item(&item).unwrap();
        }
        w.put_room_model(&RoomModel {
            id: 1,
            model_id: "model_a".into(),
            model_name: "model_a".into(),
            door_x: 0,
            door_y: 0,
            heightmap: "00000|00000|00000|00000|00000".into(),
            ..Default::default()
        })
        .unwrap();
        w.put_room(&Room {
            id: ROOM,
            owner_id: ALICE,
            name: "Alice's".into(),
            model: "model_a".into(),
            visitors_max: 25,
            ..Default::default()
        })
        .unwrap();
        w.set_item_auto_increment(1).unwrap();
        w.commit().unwrap();
        for id in [ALICE, BOB] {
            store
                .put_statistics(id, &UserStatistics::new_row())
                .unwrap();
        }
        Self {
            world: World::new(store).unwrap(),
            _dir: dir,
        }
    }

    pub fn store(&self) -> &Store {
        &self.world.store
    }

    /// A connection logged in as `user_id` (the parts of `player::login` the handlers need),
    /// registered with the world.
    pub fn client(&self, user_id: u32) -> Client {
        let (tx, rx) = unbounded_channel();
        let id = u64::from(user_id) * 100 + rand::random::<u8>() as u64;
        let session = Arc::new(Session::new(id, tx));
        {
            let mut p = session.player.lock();
            p.details = self.store().user(user_id).unwrap();
            p.logged_in = true;
            p.inventory = Inventory::load(self.store(), &self.world.items, user_id);
        }
        self.world.register_login(&session, user_id);
        Client { session, rx }
    }

    /// Everything the purchase, gift, club and trade flows write, to compare before and after.
    pub fn snapshot(&self) -> Snapshot {
        let store = self.store();
        let mut pets = Vec::new();
        let mut items = Vec::new();
        for user in [ALICE, BOB] {
            for item in store.inventory_items(user).unwrap() {
                pets.push(store.pet_by_item(item.id).unwrap());
                items.push(item);
            }
        }
        items.extend(store.room_items(ROOM as i32).unwrap());
        Snapshot {
            users: [ALICE, BOB].map(|id| store.user(id).unwrap()).to_vec(),
            items,
            pets,
            transactions: store.transaction_count().unwrap(),
            statistics: [ALICE, BOB]
                .map(|id| store.statistics(id).unwrap())
                .to_vec(),
        }
    }

    /// Adds an item row and returns it.
    pub fn add_item(&self, owner: u32, definition_id: u32, custom_data: &str, room: u32) -> Item {
        let mut w = self.store().write().unwrap();
        let mut item = w
            .insert_item(
                &Item::with_defaults(owner, definition_id, custom_data.into(), 0),
                0,
            )
            .unwrap();
        if room != 0 {
            item.room_id = room as i32;
            w.put_item(&item).unwrap();
        }
        w.commit().unwrap();
        item
    }
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
    users: Vec<Option<User>>,
    items: Vec<Item>,
    pets: Vec<Option<havana_storage::Pet>>,
    transactions: u64,
    statistics: Vec<Option<UserStatistics>>,
}

pub struct Client {
    pub session: Arc<Session>,
    rx: UnboundedReceiver<Outgoing>,
}

impl Client {
    /// The headers of the frames queued since the last call.
    pub fn headers(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            if let Outgoing::Frame(f) = msg {
                out.push((u32::from(f[0]) - 64) << 6 | (u32::from(f[1]) - 64));
            }
        }
        out
    }

    /// Calls a handler as if this client had sent `body`.
    pub fn send(
        &self,
        world: &World,
        handler: fn(&World, &Arc<Session>, &mut ClientReader) -> Result<(), Malformed>,
        body: havana_protocol::body::ClientBody,
    ) {
        let body = body.build();
        handler(world, &self.session, &mut ClientReader::new(&body)).unwrap();
    }

    pub fn credits(&self) -> i32 {
        self.session.player.lock().details.as_ref().unwrap().credits
    }

    /// Row ids of the items in the hand.
    pub fn hand(&self) -> Vec<u64> {
        self.session.player.lock().inventory.record_ids()
    }
}
