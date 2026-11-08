//! Tests of the Java string helpers the `IIM` parsing relies on, and of game flows the traces
//! cannot cover (chess: the oracle has no chesslib; poker: random cards).

use super::{java_parse_int, java_replace, java_split, java_trim};

#[test]
fn split_like_java() {
    assert_eq!(java_split("", ' '), vec![""]);
    assert_eq!(java_split("abc", ' '), vec!["abc"]);
    assert_eq!(java_split("a b  c ", ' '), vec!["a", "b", "", "c"]);
    assert_eq!(java_split(" a", ' '), vec!["", "a"]);
    assert!(java_split("   ", ' ').is_empty());
}

#[test]
fn replace_trim_and_parse_like_java() {
    assert_eq!(java_replace("id CMD x id CMD", "id CMD", ""), " x ");
    assert_eq!(java_trim("\t x \r"), "x");
    assert_eq!(java_parse_int("+5").ok(), Some(5));
    assert_eq!(java_parse_int("-5").ok(), Some(-5));
    assert!(java_parse_int("").is_err());
    assert!(java_parse_int("1.0").is_err());
}

mod flows {
    use std::sync::Arc;

    use tokio::sync::mpsc::UnboundedReceiver;

    use crate::game::item::roller::tests::model_a;
    use crate::game::pathfinder::Position;
    use crate::game::room::Room;
    use crate::game::room::items::tests::{definition, flat_item};
    use crate::session::{Outgoing, Session};
    use crate::world::World;

    pub fn world() -> (tempfile::TempDir, Arc<World>) {
        let dir = tempfile::tempdir().unwrap();
        let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, Arc::new(World::new(store).unwrap()))
    }

    /// The bodies of the messages with `header` queued since the last call, as text.
    pub fn bodies(rx: &mut UnboundedReceiver<Outgoing>) -> Vec<(u16, String)> {
        let mut out = Vec::new();
        while let Ok(Outgoing::Frame(f)) = rx.try_recv() {
            let header = rusbbo_protocol::encoding::b64_decode(&f[..2]) as u16;
            let body = f[2..f.len() - 1].iter().map(|&b| b as char).collect();
            out.push((header, body));
        }
        out
    }

    pub struct Table {
        pub room: Room,
        pub sessions: Vec<Arc<Session>>,
        pub rx: Vec<UnboundedReceiver<Outgoing>>,
        pub items: Vec<usize>,
    }

    /// A flat on model_a with chairs of `interactor` at `chairs` and a user on each.
    pub fn table(world: &Arc<World>, interactor: &str, chairs: &[(i32, i32)]) -> Table {
        let def = definition(
            (1, "gamehall_chair_green", 1),
            (1, 1, 1.0),
            0,
            "can_sit_on_top",
            interactor,
            "0,2,4,6",
        );
        let items = chairs
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| flat_item(i as i32 + 1, def.clone(), (x, y, 0)))
            .collect();
        let mut room = Room::for_test(1, model_a(), items);
        room.world = Arc::downgrade(world);
        let mut t = Table {
            room,
            sessions: Vec::new(),
            rx: Vec::new(),
            items: Vec::new(),
        };
        for (i, &(x, y)) in chairs.iter().enumerate() {
            let (e, rx) = t.room.add_test_user(Position::new(5 + i as i32, 12));
            t.room.entities[e].details.name = format!("bot{}", i + 1);
            t.room.entities[e].details.user_id = i as u32 + 1;
            t.sessions.push(t.room.entities[e].session.clone());
            t.rx.push(rx);
            t.items.push(
                t.room
                    .mapping
                    .tile(Position::new(x, y))
                    .unwrap()
                    .highest_item
                    .unwrap(),
            );
        }
        t
    }

    impl Table {
        /// Moves the user from where it waits onto its chair and runs the chair's trigger.
        pub fn sit(&mut self, player: usize) {
            let chair = self.room.mapping.items[self.items[player]].position;
            let e = &mut self.room.entities[player];
            let (from, id) = (e.position, e.instance_id);
            e.position = Position::new(chair.x, chair.y);
            self.room.mapping.remove_entity(from, id);
            self.room
                .mapping
                .add_entity(Position::new(chair.x, chair.y), id);
            self.room.invoke_item_for_test(player);
        }

        pub fn game_id(&self) -> String {
            self.room
                .gamehall
                .instances
                .iter()
                .find_map(|g| g.game_id.clone())
                .unwrap()
        }

        pub fn send(&mut self, world: &World, player: usize, command: &str) {
            let contents = format!("{} {command}", self.game_id());
            let session = self.sessions[player].clone();
            self.room
                .game_command(world, &session, self.items[player], &contents);
        }

        pub fn received(&mut self, player: usize) -> Vec<(u16, String)> {
            bodies(&mut self.rx[player])
        }
    }

    #[test]
    fn poker_round_of_two_ends_in_a_reveal_only_on_a_tie() {
        let (_dir, world) = world();
        let mut t = table(&world, "game_poker", &[(8, 2), (8, 4)]);
        t.sit(0);
        t.sit(1);
        for p in 0..2 {
            t.send(&world, p, "STARTOVER");
        }
        t.send(&world, 0, "CHANGE");
        t.received(0);
        t.received(1);
        t.send(&world, 1, "CHANGE 1 2");
        let got = t.received(0);
        let chat = got
            .iter()
            .find(|(h, _)| *h == 24)
            .expect("the winner's line");
        let revealed = got.iter().any(|(_, b)| b.contains("REVEALCARDS"));
        // With the default settings, Java's rewardRares throws for a single winner (the empty
        // poker.reward.rares has no catalogue item): no REVEALCARDS, the round stays.
        if chat.1.contains("wins this round") {
            assert!(!revealed, "{got:?}");
        } else {
            assert!(
                chat.1.contains("It's a tie! bot1, bot2 all have"),
                "{chat:?}"
            );
            assert!(revealed, "{got:?}");
        }
    }

    #[test]
    fn chess_from_the_choice_of_side_to_mate() {
        let (_dir, world) = world();
        let mut t = table(&world, "game_chess", &[(7, 3), (9, 3)]);
        t.sit(0);
        assert!(
            t.room
                .gamehall
                .instances
                .iter()
                .all(|g| g.game_id.is_none())
        );
        t.sit(1);
        let id = t.game_id();
        for p in 0..2 {
            let got = t.received(p);
            assert!(got.contains(&(145, format!("{id}\tChess\t"))), "{got:?}");
        }
        t.send(&world, 1, "MOVEPIECE e2 e4");
        // Nobody's turn before the sides are chosen: TYPERESERVED and the board.
        let got = t.received(1);
        assert_eq!(got[0], (144, format!("{id}\r\rTYPERESERVED\r\r")));
        assert!(got[1].1.contains("PIECEDATA\r\r\r\r\r\rWtwa1 Whrb1"));
        t.received(0);

        t.send(&world, 0, "CHOOSETYPE w");
        let got = t.received(0);
        assert_eq!(got[0], (144, format!("{id}\r\rSELECTTYPE w\r\r")));
        assert!(
            got[1]
                .1
                .starts_with(&format!("{id}\r\rPIECEDATA\r\rW bot1\r\r\r\rWtwa1 "))
        );
        assert_eq!(t.received(1)[0], (144, format!("{id}\r\rSELECTTYPE b\r\r")));
        t.send(&world, 1, "CHOOSETYPE w");
        assert_eq!(t.received(1)[0], (144, format!("{id}\r\rTYPERESERVED\r\r")));

        for (p, m) in [(0, "f2 f3"), (1, "e7 e5"), (0, "g2 g4")] {
            t.send(&world, p, &format!("MOVEPIECE {m}"));
        }
        t.received(0);
        t.received(1);
        t.send(&world, 1, "MOVEPIECE e5 e3");
        let got = t.received(1);
        assert_eq!(got.len(), 1, "an illegal move: the board again");
        assert!(got[0].1.contains("PIECEDATA\r\rB bot2\r\r"));
        t.received(0);
        t.send(&world, 1, "MOVEPIECE D8 H4");
        for p in 0..2 {
            let got = t.received(p);
            assert_eq!(got[0].0, 24);
            assert!(
                got[0].1.ends_with("bot2 has won the chess game\x02H"),
                "{got:?}"
            );
            assert!(got[1].1.contains("Bquh4 "));
        }
        t.send(&world, 1, "MOVEPIECE h4 e1");
        assert_eq!(t.received(1)[0], (144, format!("{id}\r\rTYPERESERVED\r\r")));
        t.send(&world, 0, "RESTART");
        assert!(
            t.received(1)[0]
                .1
                .contains("PIECEDATA\r\rW bot1\r\r\r\rWtwa1 ")
        );
        t.received(0);
        t.send(&world, 0, "MOVEPIECE e2 e9");
        assert!(t.received(0).is_empty(), "no such square: Java throws");

        t.send(&world, 0, "CLOSE");
        for p in 0..2 {
            assert_eq!(t.received(p), vec![(146, format!("{id}\tChess\t"))]);
        }
        // Java only clears the leaver's game id; the other player keeps the old one.
        assert!(t.sessions[0].current_game_id.lock().is_none());
        assert_eq!(
            t.sessions[1].current_game_id.lock().as_deref(),
            Some(id.as_str())
        );
    }
}
