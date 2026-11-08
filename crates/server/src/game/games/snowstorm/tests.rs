//! SnowStorm engine tests. The byte strings marked "oracle" come from a recording of the Java
//! server (two bots, arena 1, two teams: tracebot1 spawned at 43,28 facing 4, tracebot2 at
//! 30,42 facing 3; the machine at 25,24 is object 1, the avatars 2 and 3); `testdata/` holds
//! that recording's arena `HEIGHTMAP`, `OBJECTS_WORLD` and `FULLGAMESTATUS` bodies.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::{ClientBody, ClientReader};

use super::attributes::ActivityState;
use super::events::TurnEvent;
use super::game::{Output, PlayerInfo, SnowStormGame, circle, length_choice_seconds};
use super::mapping::java_split;
use super::maps::{self, SnowStormMaps};
use super::objects::{Snowball, Trajectory};
use super::outgoing::{self, snowstorm_gamestatus};
use super::*;

const T0: i64 = 1_700_000_000_000;

fn bot(user_id: u32, team_id: i32) -> PlayerInfo {
    let (name, motto, figure, sex) = match user_id {
        1 => (
            "tracebot1",
            "trace bot one",
            "hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61",
            "M",
        ),
        _ => (
            "tracebot2",
            "trace bot two",
            "hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33",
            "F",
        ),
    };
    PlayerInfo {
        user_id,
        team_id,
        name: name.into(),
        motto: motto.into(),
        figure: figure.into(),
        sex: sex.into(),
    }
}

/// A game with its arena task, which runs a turn every 300 ms from `next_tick` (the server's
/// arena task calls [`SnowStormGame::tick`] the same way).
struct Arena {
    game: SnowStormGame,
    next_tick: i64,
}

impl std::ops::Deref for Arena {
    type Target = SnowStormGame;
    fn deref(&self) -> &SnowStormGame {
        &self.game
    }
}

impl std::ops::DerefMut for Arena {
    fn deref_mut(&mut self) -> &mut SnowStormGame {
        &mut self.game
    }
}

impl Arena {
    /// Every turn due up to `to`, then the timers up to `to`.
    fn run_until(&mut self, to: i64) -> Vec<Output> {
        let mut out = Vec::new();
        while self.next_tick <= to {
            out.extend(self.game.tick(self.next_tick));
            self.next_tick += 300;
        }
        self.game.advance(to);
        out
    }
}

/// The recorded game: arena 1, tracebot1 (team 0) and tracebot2 (team 1) placed where Java
/// spawned them, both in the arena room, the turn task running from `T0`.
fn recorded_game() -> Arena {
    let mut game = SnowStormGame::new(1, 2, 1, &SnowStormMaps::embedded());
    game.add_player(bot(1, 0));
    game.add_player(bot(2, 1));
    game.initialise();
    let spawns = game.assign_spawn_points(&mut |_| 0, T0);
    assert_eq!(
        spawns.iter().map(|s| s.object_id).collect::<Vec<_>>(),
        [2, 3]
    );
    place(&mut game, 1, 43, 28, 4);
    place(&mut game, 2, 30, 42, 3);
    for user in [1, 2] {
        game.set_in_room(user, true);
    }
    game.start_external_turns();
    Arena {
        game,
        next_tick: T0,
    }
}

fn place(game: &mut SnowStormGame, user_id: u32, x: i32, y: i32, rotation: i32) {
    let a = &mut game.player_mut(user_id).unwrap().attrs;
    a.current_position = crate::game::pathfinder::Position::new(x, y);
    a.rotation = rotation;
}

fn broadcasts(out: Vec<Output>) -> Vec<Vec<u8>> {
    out.into_iter()
        .filter_map(|o| match o {
            Output::Broadcast(Message { header, body }) => {
                assert_eq!(header, 244);
                Some(body)
            }
            Output::SpamTick(_) => None,
        })
        .collect()
}

/// Every turn body from `from` to `to` (inclusive), turn by turn.
fn turns_until(game: &mut Arena, to: i64) -> Vec<Vec<u8>> {
    broadcasts(game.run_until(to))
}

fn event(game: &mut Arena, user_id: u32, body: ClientBody, now: i64) -> Vec<Vec<u8>> {
    let bytes = body.build();
    let mut reader = ClientReader::new(&bytes);
    let event_type = reader.int().unwrap();
    game.handle_event(user_id, event_type, &mut reader, true, now);
    Vec::new()
}

const EMPTY_TURN: &[u8] = b"IIQAHHHHH";

fn non_empty(turns: &[Vec<u8>]) -> Vec<&[u8]> {
    turns
        .iter()
        .map(Vec::as_slice)
        .filter(|t| *t != EMPTY_TURN)
        .collect()
}

#[test]
fn every_arena_loads_from_the_files_like_java() {
    let embedded = SnowStormMaps::embedded();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/snowstorm_maps");
    let from_dir = SnowStormMaps::load_dir(&dir);
    for id in maps::MAP_IDS {
        let a = embedded.get(id).expect("embedded arena");
        let b = from_dir.get(id).expect("arena from the directory");
        assert_eq!(a.items(), b.items());
        assert_eq!(a.heightmap(), b.heightmap());
        assert_eq!(a.spawn_clusters(), b.spawn_clusters());
    }
    assert!(embedded.get(8).is_none());

    let one = embedded.get(1).unwrap();
    assert_eq!((one.size_x(), one.size_y()), (50, 50));
    // The default heightmap's first row is one tile short, so arenas 4-6 are 49 wide.
    let four = embedded.get(4).unwrap();
    assert_eq!((four.size_x(), four.size_y()), (49, 50));
    assert_eq!(
        one.spawn_clusters()[0],
        mapping::SnowStormSpawn {
            x: 30,
            y: 41,
            radius: 8,
            min_distance: 4
        }
    );
    // A machine is three items; only the left one is the machine, all three block.
    let machine = one.tile(Position::new(25, 24)).unwrap();
    assert!(one.highest_item(machine).unwrap().is_snowball_machine());
    for x in 25..=27 {
        assert!(!one.tile(Position::new(x, 24)).unwrap().is_walkable());
    }
    assert!(one.tile(Position::new(25, 25)).unwrap().is_walkable());
}

#[test]
fn a_missing_arena_file_leaves_the_arena_out() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("arena_2.dat"), "a1 sw_tree1 3 4 0 0").unwrap();
    let maps = SnowStormMaps::load_dir(dir.path());
    assert!(maps.get(1).is_none());
    let two = maps.get(2).unwrap();
    assert_eq!(two.items().len(), 1);
    assert_eq!(two.items()[0].height, 3);
    assert!(two.spawn_clusters().is_empty());
    // Java's parseInt failure is an error for that arena only.
    assert!(maps::parse_map(3, "a1 sw_tree1 x 4 0 0", None, None).is_err());
}

#[test]
fn java_split_drops_trailing_empty_strings() {
    assert_eq!(java_split("1 2 3 4|5 6 7 8|", '|'), ["1 2 3 4", "5 6 7 8"]);
    assert_eq!(java_split("abc", '|'), ["abc"]);
    assert_eq!(java_split("", '|'), [""]);
    assert!(java_split("||", '|').is_empty());
    assert_eq!(java_split("a||b", '|'), ["a", "", "b"]);
}

#[test]
fn item_heights_follow_java() {
    assert_eq!(maps::item_height("sw_tree3"), 3);
    assert_eq!(maps::item_height("block_ice2"), 2);
    assert_eq!(maps::item_height("sw_fence"), 1);
    assert_eq!(maps::item_height("block_small"), 0);
}

#[test]
fn arena_heightmap_matches_the_oracle() {
    let map = SnowStormMaps::embedded().get(1).unwrap();
    let msg = outgoing::heightmap(&map);
    assert_eq!(msg.header, 31);
    assert_eq!(msg.body, include_bytes!("testdata/arena1_heightmap.bin"));
}

#[test]
fn arena_objects_world_matches_the_oracle() {
    let map = SnowStormMaps::embedded().get(1).unwrap();
    let expected = include_bytes!("testdata/arena1_objects_world.bin");
    // The ids are random in Java: take them from the recording at the places ours go.
    let placeholder = outgoing::objects_world(&map, || "########".into()).unwrap();
    let mut ids = Vec::new();
    let mut i = 0;
    while let Some(at) = placeholder.body[i..]
        .windows(8)
        .position(|w| w == b"########")
    {
        let at = i + at;
        ids.push(String::from_utf8(expected[at..at + 8].to_vec()).unwrap());
        i = at + 8;
    }
    assert_eq!(ids.len(), 88);
    let mut ids = ids.into_iter();
    let msg = outgoing::objects_world(&map, || ids.next().unwrap()).unwrap();
    assert_eq!(msg.header, 30);
    assert_eq!(msg.body, expected);
    assert_eq!(outgoing::random_object_id().len(), 8);
}

#[test]
fn full_game_status_matches_the_oracle() {
    let game = recorded_game();
    // Bot a asked while 9 of the 10 preparing seconds were left; the task had not queued
    // anything yet.
    let msg = game.full_game_status(1, 9, 10, T0);
    assert_eq!(msg.header, 243);
    assert_eq!(
        msg.body,
        include_bytes!("testdata/arena1_fullgamestatus.bin")
    );
    // Without the arena task, five empty turns as well.
    let mut game = game;
    game.stop_turns();
    assert_eq!(
        game.full_game_status(1, 9, 10, T0).body,
        include_bytes!("testdata/arena1_fullgamestatus.bin")
    );
}

#[test]
fn walking_to_the_machine_matches_the_oracle() {
    let mut game = recorded_game();
    assert_eq!(turns_until(&mut game, T0), [EMPTY_TURN]);
    // Bot a walks to 25,25 (world 80000, 80000), in front of the machine.
    let none = event(
        &mut game,
        1,
        ClientBody::new().int(0).int(80000).int(80000),
        T0 + 100,
    );
    assert!(none.is_empty());
    let turns = turns_until(&mut game, T0 + 300 * 22);
    // Oracle: 22 turns with the move event (the last one on arrival) ...
    assert_eq!(turns, vec![b"IIQAIJJ``xD``xDHHHH".to_vec(); 22]);
    let a = &game.player(1).unwrap().attrs;
    assert_eq!(a.current_position, Position::new(25, 25));
    assert!(!a.walking);
    assert_eq!(a.rotation, 0);
    // ... then the machine fills one snowball (oracle), every 3 s while someone stands there.
    assert_eq!(turns_until(&mut game, T0 + 300 * 23), [b"IIQAISBIHHHH"]);
    let turns = turns_until(&mut game, T0 + 300 * 23 + 3000);
    assert_eq!(non_empty(&turns), [b"IIQAISBIHHHH"]);
    assert_eq!(game.machines[0].snowballs, 2);
    // A full player takes nothing; one with room gets one a second (oracle: event 12).
    game.player_mut(1).unwrap().attrs.snowballs = 4;
    let turns = turns_until(&mut game, T0 + 300 * 23 + 4000);
    assert_eq!(non_empty(&turns), [b"IIQAIPCJIHHHH"]);
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 5);
    assert_eq!(game.machines[0].snowballs, 1);
    // Leaving the tile stops both machine timers.
    place(&mut game, 1, 25, 26, 0);
    let turns = turns_until(&mut game, T0 + 300 * 23 + 10_000);
    assert!(non_empty(&turns).is_empty());
    assert!(
        game.timers
            .iter()
            .all(|(_, k)| matches!(k, timers::TimerKind::ActivityReset { .. }))
    );
}

#[test]
fn throws_match_the_oracle() {
    let mut game = recorded_game();
    place(&mut game, 1, 25, 25, 0);
    let t = T0 + 1000;
    turns_until(&mut game, t);

    // a throws long at b (object 3): snowball 4 (oracle).
    event(&mut game, 1, ClientBody::new().int(1).int(3).int(2), t + 10);
    assert_eq!(
        turns_until(&mut game, t + 300),
        [b"IIQAJPAJ`@wE`@MHJPBPAJ`@wE`@MHJHHHH"]
    );
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 4);

    // Within 300 ms of a throw, nothing.
    event(
        &mut game,
        1,
        ClientBody::new().int(2).int(99000).int(64000).int(1),
        t + 200,
    );
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 4);

    // a throws short at tile 30,20: snowball 5 (oracle).
    event(
        &mut game,
        1,
        ClientBody::new().int(2).int(99000).int(64000).int(1),
        t + 400,
    );
    assert_eq!(
        turns_until(&mut game, t + 600),
        [b"IIQAJPAJ`@wE`@zCIPBQAJ`@wE`@zCIHHHH"]
    );

    // b throws quick at a (object 2): snowball 6 (oracle).
    event(
        &mut game,
        2,
        ClientBody::new().int(1).int(2).int(0),
        t + 700,
    );
    assert_eq!(
        turns_until(&mut game, t + 900),
        [b"IIQAJPAK``xD``xDHPBRAK``xD``xDHHHHH"]
    );

    // The long ball (18 tenths of a second, steps of 100 ms) hits b in the first step past
    // its time: hit (direction 3) and delete (oracle, without the machine's event here).
    let turns = turns_until(&mut game, t + 2100);
    assert_eq!(non_empty(&turns), [b"IIQAJQAJKKIPAHHHH"]);
    // The quick ball reaches a's tile in its 18th step: hit (direction 7) and delete (oracle).
    let turns = turns_until(&mut game, t + 2700);
    assert_eq!(non_empty(&turns), [b"IIQAJQAKJSAIRAHHHH"]);

    let (a, b) = (game.player(1).unwrap(), game.player(2).unwrap());
    assert_eq!((a.attrs.score, a.attrs.health), (1, 3));
    assert_eq!((b.attrs.score, b.attrs.health), (1, 3));
    assert!(game.flights.is_empty());
    // The short ball landed on an empty tile without being blocked: no delete.
    assert_eq!(game.final_scores(), [(1, 1), (2, 1)]);
}

#[test]
fn turn_bodies_serialise_each_event() {
    let turns = vec![
        vec![
            TurnEvent::AvatarMove {
                object_id: 2,
                x: 80000,
                y: 80000,
            },
            TurnEvent::CreateSnowball { object_id: 2 },
            TurnEvent::DeleteObject { object_id: 6 },
            TurnEvent::Hit {
                thrower_id: 3,
                target_id: 2,
                direction: 7,
            },
            TurnEvent::LaunchSnowball {
                object_id: 4,
                thrower_id: 2,
                x: 96000,
                y: 134400,
                trajectory: 2,
            },
            TurnEvent::MachineAddSnowball { machine_id: 1 },
            TurnEvent::MachineMoveSnowballs {
                player_id: 2,
                machine_id: 1,
            },
            TurnEvent::Stun {
                stunned_id: 3,
                thrower_id: 2,
                direction: 135,
            },
            TurnEvent::Throw {
                object_id: 2,
                x: 96000,
                y: 64000,
                trajectory: 1,
            },
        ],
        vec![],
        vec![],
        vec![],
        vec![],
    ];
    let body = snowstorm_gamestatus(&turns).body;
    let expected: &[u8] = b"IIQA\
        QBJJ``xD``xD\
        SAJ\
        IRA\
        QAKJSA\
        PBPAJ`@wE`@MHJ\
        SBI\
        PCJI\
        QBKJSa\
        PAJ`@wE`@zCI\
        HHHH";
    assert_eq!(body, expected);
    // An empty list still says one turn (Java: `turns.size() == 0 ? 1 : size`).
    assert_eq!(snowstorm_gamestatus(&[]).body, b"III");
}

#[test]
fn four_hits_stun_then_invincible_then_normal() {
    let mut game = recorded_game();
    place(&mut game, 1, 10, 30, 0);
    place(&mut game, 2, 12, 30, 0);
    game.player_mut(1).unwrap().attrs.snowballs = 5;
    let mut t = T0 + 1000;
    turns_until(&mut game, t);
    let mut last_turn = Vec::new();
    for hit in 1..=4 {
        // Trajectory 3: Java has no trajectory for it, so the ball lands at once, no delete.
        event(&mut game, 1, ClientBody::new().int(1).int(3).int(3), t);
        let turns = turns_until(&mut game, t + 300);
        let events = non_empty(&turns);
        assert_eq!(events.len(), 1);
        last_turn = events[0].to_vec();
        t += 400;
        if hit < 4 {
            assert_eq!(game.player(2).unwrap().attrs.health, 4 - hit);
        }
    }
    let b = &game.player(2).unwrap().attrs;
    assert_eq!(b.activity_state, ActivityState::Stunned);
    assert_eq!((b.health, b.snowballs), (4, 0));
    assert_eq!(game.player(1).unwrap().attrs.score, 4 + 5);
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 1);

    // The last throw's turn: throw, launch (snowball 7), hit (direction 2), stun (45 * 2).
    let expected = snowstorm_gamestatus(&[
        vec![
            TurnEvent::Throw {
                object_id: 2,
                x: 38400,
                y: 96000,
                trajectory: 3,
            },
            TurnEvent::LaunchSnowball {
                object_id: 7,
                thrower_id: 2,
                x: 38400,
                y: 96000,
                trajectory: 3,
            },
            TurnEvent::Hit {
                thrower_id: 2,
                target_id: 3,
                direction: 2,
            },
            TurnEvent::Stun {
                stunned_id: 3,
                thrower_id: 2,
                direction: 90,
            },
        ],
        vec![],
        vec![],
        vec![],
        vec![],
    ]);
    assert_eq!(last_turn, expected.body);
    let stunned_at = t - 400;

    // Stunned: not walkable, not damageable, timer counts from twice its length.
    let b = &game.player(2).unwrap().attrs;
    assert!(!b.is_walkable());
    assert_eq!(b.activity_timer(stunned_at), (15_000 / 300) * 5);
    assert_eq!(b.activity_timer(stunned_at + 7_500), (7_500 / 300) * 5);
    assert_eq!(b.activity_timer(stunned_at + 7_501), 0);
    // A stunned player is not hit and does not block.
    event(&mut game, 1, ClientBody::new().int(1).int(3).int(3), t);
    assert_eq!(game.player(2).unwrap().attrs.health, 4);

    // 7.5 s after the stun: invincible for 3.6 s (walkable, not damageable), then normal.
    game.advance(stunned_at + 7_500);
    let b = &game.player(2).unwrap().attrs;
    assert_eq!(b.activity_state, ActivityState::InvincibleAfterStun);
    assert!(b.is_walkable() && !b.is_damageable(stunned_at + 7_500));
    assert_eq!(b.immunity_expiry, stunned_at + 7_500 + 3_600);
    game.advance(stunned_at + 7_500 + 3_600);
    let b = &game.player(2).unwrap().attrs;
    assert_eq!(b.activity_state, ActivityState::Normal);
    assert!(!b.is_damageable(stunned_at + 7_500 + 3_600));
    assert!(b.is_damageable(stunned_at + 7_500 + 3_601));
}

#[test]
fn making_a_snowball_takes_1200_ms() {
    let mut game = recorded_game();
    turns_until(&mut game, T0);
    // Full: nothing.
    event(&mut game, 1, ClientBody::new().int(3), T0 + 10);
    assert_eq!(turns_until(&mut game, T0 + 300), [EMPTY_TURN]);

    game.player_mut(1).unwrap().attrs.snowballs = 2;
    game.player_mut(1).unwrap().attrs.walking = true;
    event(&mut game, 1, ClientBody::new().int(3), T0 + 310);
    assert_eq!(turns_until(&mut game, T0 + 600), [b"IIQAISAJHHHH"]);
    let a = &game.player(1).unwrap().attrs;
    assert_eq!(a.activity_state, ActivityState::Creating);
    assert!(!a.walking && !a.is_walkable());
    // Busy: no walk, no second snowball, no throw.
    event(&mut game, 1, ClientBody::new().int(3), T0 + 700);
    event(
        &mut game,
        1,
        ClientBody::new().int(1).int(3).int(2),
        T0 + 700,
    );
    game.advance(T0 + 1509);
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 2);
    game.advance(T0 + 1510);
    let a = &game.player(1).unwrap().attrs;
    assert_eq!(a.snowballs, 3);
    assert_eq!(a.activity_state, ActivityState::Normal);
}

#[test]
fn throw_requests_are_checked_like_java() {
    let mut game = recorded_game();
    let t = T0 + 1000;
    turns_until(&mut game, t);
    // Bad trajectories.
    event(&mut game, 1, ClientBody::new().int(1).int(3).int(1), t);
    event(
        &mut game,
        1,
        ClientBody::new().int(2).int(0).int(0).int(0),
        t,
    );
    // Not an opponent (itself), or no such object.
    event(&mut game, 1, ClientBody::new().int(1).int(2).int(2), t);
    event(&mut game, 1, ClientBody::new().int(1).int(99).int(2), t);
    // A body cut short.
    event(&mut game, 1, ClientBody::new().int(2).int(5), t);
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 5);
    assert_eq!(game.player(1).unwrap().attrs.last_throw, 0);
    // No snowballs.
    game.player_mut(1).unwrap().attrs.snowballs = 0;
    event(&mut game, 1, ClientBody::new().int(1).int(3).int(2), t);
    assert_eq!(turns_until(&mut game, t + 300), [EMPTY_TURN]);
    // Unknown event types and players are ignored.
    event(&mut game, 1, ClientBody::new().int(9), t);
    event(&mut game, 77, ClientBody::new().int(3), t);
    // In a one-team game everybody else is an opponent.
    let solo = SnowStormGame::new(1, 1, 1, &SnowStormMaps::embedded());
    assert!(solo.is_opposition((1, 0), (2, 0)));
    assert!(!solo.is_opposition((1, 0), (1, 0)));
    assert!(!game.is_opposition((1, 0), (2, 0)));
}

#[test]
fn a_quick_throw_from_too_far_never_lands() {
    let mut game = recorded_game();
    place(&mut game, 1, 3, 19, 0);
    place(&mut game, 2, 3, 48, 0);
    turns_until(&mut game, T0);
    event(
        &mut game,
        1,
        ClientBody::new().int(1).int(3).int(0),
        T0 + 10,
    );
    // 30 tiles: past the 22-tile limit, the events go out but the ball has no flight.
    assert!(game.flights.is_empty());
    turns_until(&mut game, T0 + 10_000);
    assert_eq!(game.player(2).unwrap().attrs.health, 4);
    assert_eq!(game.player(1).unwrap().attrs.snowballs, 4);
}

#[test]
fn a_tree_blocks_short_throws_but_not_long_ones() {
    let map = SnowStormMaps::embedded().get(1).unwrap();
    // Arena 1 has a tree (height 3) at 22,3.
    let tree = map.tile(Position::new(22, 3)).unwrap();
    assert_eq!(map.highest_item(tree).unwrap().item_name, "sw_tree1");
    assert!(map.is_height_blocking(tree, Some(Trajectory::Quick)));
    assert!(map.is_height_blocking(tree, Some(Trajectory::Short)));
    assert!(!map.is_height_blocking(tree, Some(Trajectory::Long)));
    assert!(!map.is_height_blocking(tree, None));

    let mut game = recorded_game();
    place(&mut game, 1, 22, 6, 0);
    turns_until(&mut game, T0);
    // A short throw at 22,1 stops at the tree: the target moves there (twice at the end of the
    // line, so the launch says 22,3), and the landing (within the same turn) deletes the ball.
    event(
        &mut game,
        1,
        ClientBody::new().int(2).int(22 * 3200).int(3200).int(1),
        T0 + 10,
    );
    let expected = snowstorm_gamestatus(&[
        vec![
            TurnEvent::Throw {
                object_id: 2,
                x: 22 * 3200,
                y: 3 * 3200,
                trajectory: 1,
            },
            TurnEvent::LaunchSnowball {
                object_id: 4,
                thrower_id: 2,
                x: 22 * 3200,
                y: 3 * 3200,
                trajectory: 1,
            },
            TurnEvent::DeleteObject { object_id: 4 },
        ],
        vec![],
        vec![],
        vec![],
        vec![],
    ]);
    assert_eq!(turns_until(&mut game, T0 + 300), [expected.body]);
    let turns = turns_until(&mut game, T0 + 3000);
    assert!(non_empty(&turns).is_empty());

    // A long one flies over to 22,1 and lands without a delete.
    event(
        &mut game,
        1,
        ClientBody::new().int(2).int(22 * 3200).int(3200).int(2),
        T0 + 3000,
    );
    let turns = turns_until(&mut game, T0 + 6000);
    assert_eq!(non_empty(&turns).len(), 1); // only the throw and launch
}

#[test]
fn the_visible_line_is_bresenham_with_the_block_twice() {
    let mut game = recorded_game();
    place(&mut game, 1, 22, 6, 0);
    place(&mut game, 2, 40, 40, 0);
    let ball = |trajectory_id, tx, ty| Snowball {
        object_id: 99,
        thrower: 1,
        thrower_object_id: 2,
        thrower_team: 0,
        from_x: 22,
        from_y: 6,
        target_x: tx,
        target_y: ty,
        trajectory_id,
        direction: 0,
        blocked: false,
        target_player: None,
    };
    let line = game.snowball_path(&ball(1, 22, 1));
    let tiles: Vec<(i32, i32)> = line.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(tiles, [(22, 6), (22, 5), (22, 4), (22, 3), (22, 3)]);
    let line = game.snowball_path(&ball(2, 22, 1));
    assert_eq!(line.len(), 6);
    let line = game.snowball_path(&ball(2, 25, 8));
    let tiles: Vec<(i32, i32)> = line.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(tiles, [(22, 6), (23, 7), (24, 7), (25, 8)]);
    // An opponent in the way stops all but long throws.
    place(&mut game, 2, 23, 7, 0);
    assert_eq!(game.snowball_path(&ball(0, 25, 8)).len(), 3);
    assert_eq!(game.snowball_path(&ball(2, 25, 8)).len(), 4);
    game.player_mut(2).unwrap().attrs.activity_state = ActivityState::Stunned;
    assert_eq!(game.snowball_path(&ball(0, 25, 8)).len(), 4);
}

#[test]
fn time_to_live_follows_java() {
    let ball = |tx, ty, trajectory_id| Snowball {
        object_id: 1,
        thrower: 1,
        thrower_object_id: 1,
        thrower_team: 0,
        from_x: 25,
        from_y: 25,
        target_x: tx,
        target_y: ty,
        trajectory_id,
        direction: 0,
        blocked: false,
        target_player: None,
    };
    assert_eq!(ball(30, 42, 2).time_to_live(), 18);
    assert_eq!(ball(26, 25, 0).time_to_live(), 1);
    assert_eq!(ball(25, 25, 1).time_to_live(), 0);
    assert_eq!(ball(30, 42, 3).time_to_live(), -1);
}

#[test]
fn walking_is_greedy_and_players_do_not_block() {
    let mut game = recorded_game();
    place(&mut game, 1, 40, 30, 0);
    place(&mut game, 2, 41, 30, 0);
    game.player_mut(1).unwrap().attrs.walk_goal = Some(Position::new(45, 30));
    // Ties go to the first of DIAGONAL_MOVE_POINTS: 41,30 (east) before 41,29 and 41,31; the
    // other player standing there does not matter.
    assert_eq!(game.next_direction(0), Some(Position::new(41, 30)));
    game.player_mut(1).unwrap().attrs.walk_goal = Some(Position::new(40, 20));
    assert_eq!(game.next_direction(0), Some(Position::new(40, 29)));
    // A goal far off the map wraps like Java's int instead of panicking.
    game.player_mut(1).unwrap().attrs.walk_goal = Some(Position::new(600_000, 600_000));
    assert!(game.next_direction(0).is_some());
    assert_eq!(
        java_distance(Position::new(0, 0), Position::new(600_000, 0)),
        0
    );
    assert_eq!(java_distance(Position::new(0, 0), Position::new(3, 4)), 5);
}

#[test]
fn walk_requests_are_checked_like_java() {
    let mut game = recorded_game();
    place(&mut game, 1, 25, 25, 0);
    turns_until(&mut game, T0);
    event(
        &mut game,
        1,
        ClientBody::new().int(0).int(25 * 3200).int(24 * 3200),
        T0 + 10,
    );
    // The machine tile itself is not walkable: the walker steps around it forever or stops.
    let turns = turns_until(&mut game, T0 + 3000);
    assert!(!turns.is_empty());
    // Walking to where one already is does nothing.
    let mut game = recorded_game();
    turns_until(&mut game, T0);
    event(
        &mut game,
        1,
        ClientBody::new().int(0).int(43 * 3200 + 5).int(28 * 3200),
        T0 + 10,
    );
    assert!(!game.player(1).unwrap().attrs.walking);
    // Not allowed to walk (the room user's flag): nothing.
    let bytes = ClientBody::new().int(80000).int(80000).build();
    game.handle_event(1, 0, &mut ClientReader::new(&bytes), false, T0 + 20);
    assert!(!game.player(1).unwrap().attrs.walking);
}

#[test]
fn the_turn_task_follows_java() {
    let mut game = recorded_game();
    // Spam ticks for the players in the room, then the status.
    let out = game.run_until(T0);
    assert_eq!(
        out,
        [
            Output::SpamTick(1),
            Output::SpamTick(2),
            Output::Broadcast(Message::new(244, EMPTY_TURN.to_vec()))
        ]
    );
    // Every 300 ms.
    assert_eq!(turns_until(&mut game, T0 + 1000).len(), 3);
    // A player out of the room is not moved or ticked.
    game.set_in_room(2, false);
    assert_eq!(game.run_until(T0 + 1200).len(), 2);
    // Events queued before the turn appear in FULLGAMESTATUS and the next status.
    game.send_queue(0, 1, TurnEvent::DeleteObject { object_id: 9 });
    game.send_queue(0, 6, TurnEvent::DeleteObject { object_id: 9 }); // out of range: dropped
    assert_eq!(game.executing_turns().unwrap()[0].len(), 1);
    // A future event waits its frames, then goes in.
    game.send_queue(2, 2, TurnEvent::DeleteObject { object_id: 8 });
    assert_eq!(
        turns_until(&mut game, T0 + 1500),
        [b"IIQAIIQBHHHH".to_vec()]
    );
    assert_eq!(turns_until(&mut game, T0 + 1800), [EMPTY_TURN]);
    assert_eq!(
        turns_until(&mut game, T0 + 2100),
        [b"IIQAHIIPBHHH".to_vec()]
    );
    assert_eq!(turns_until(&mut game, T0 + 2400), [EMPTY_TURN]);
    // Nobody in the game: nothing at all.
    game.set_in_game(1, false);
    game.set_in_game(2, false);
    assert!(game.run_until(T0 + 3000).is_empty());
    // Stopping the task drops its turns; starting again starts a new task.
    game.set_in_game(1, true);
    game.send_queue(0, 1, TurnEvent::DeleteObject { object_id: 9 });
    game.stop_turns();
    assert!(game.run_until(T0 + 4000).is_empty());
    game.start_external_turns();
    assert_eq!(turns_until(&mut game, T0 + 4300).len(), 1);
}

#[test]
fn spawns_come_from_the_clusters() {
    let maps = SnowStormMaps::embedded();
    let mut game = SnowStormGame::new(1, 2, 2, &maps);
    game.add_player(bot(2, 1));
    game.add_player(bot(1, 0));
    // Team order: team 0 first.
    assert_eq!(game.players()[0].info.user_id, 1);
    game.initialise();
    let mut seq = [0, 0, 3, 1, 5, 2, 9, 4].into_iter().cycle();
    let spawns = game.assign_spawn_points(&mut |n| seq.next().unwrap() % n, T0);
    let map = maps.get(1).unwrap();
    for s in &spawns {
        assert!(map.tile(s.spawn).unwrap().is_walkable());
        assert!((0..7).contains(&s.rotation));
        let p = game.player(s.user_id).unwrap();
        assert_eq!(p.attrs.current_position, s.spawn);
        assert_eq!(
            (p.attrs.health, p.attrs.snowballs, p.attrs.score),
            (4, 5, 0)
        );
        assert!(
            map.spawn_clusters()
                .iter()
                .any(|c| { circle(Position::new(c.x, c.y), c.radius).contains(&s.spawn) })
        );
    }
    assert_eq!(game.object_count(), 3);
    // Restarting creates new objects with new ids (the counter is never reset).
    game.initialise();
    let spawns = game.assign_spawn_points(&mut |_| 0, T0);
    assert_eq!(spawns[0].object_id, 5);
    assert_eq!(game.object_count(), 3);
    // Leaving removes the avatar.
    game.remove_player(2);
    assert_eq!(game.object_count(), 2);

    // A map without clusters (or none loaded): 15,18.
    let mut game = SnowStormGame::new(9, 2, 1, &maps);
    game.add_player(bot(1, 0));
    game.initialise();
    let spawns = game.assign_spawn_points(&mut |_| 0, T0);
    assert_eq!(spawns[0].spawn, SPAWN_FALLBACK);
    assert_eq!(game.object_count(), 1);
}

#[test]
fn circle_is_javas_get_circle() {
    let c = circle(Position::new(0, 0), 1);
    assert_eq!(c.len(), 9); // the corners are at distance 1.41, truncated to 1
    assert_eq!(circle(Position::new(5, 5), 0), [Position::new(5, 5)]);
}

#[test]
fn lengths_follow_the_choice() {
    let maps = SnowStormMaps::embedded();
    let game = SnowStormGame::new(1, 2, 2, &maps);
    assert_eq!(game.lifetime_seconds(0), 180);
    assert_eq!(game.lifetime_seconds(45), 45);
    assert_eq!(length_choice_seconds(3), Some(300));
    assert_eq!(length_choice_seconds(4), None);
    let odd = SnowStormGame::new(1, 2, 9, &maps);
    assert_eq!(odd.lifetime_seconds(0), 0);
}

#[test]
fn activity_states_follow_java() {
    assert_eq!(ActivityState::Stunned.time_in_ms(), 7500);
    assert_eq!(ActivityState::InvincibleAfterStun.time_in_ms(), 3600);
    assert_eq!(ActivityState::Creating.time_in_ms(), 1200);
    assert_eq!(ActivityState::Normal.time_in_ms(), 0);
    assert_eq!(
        [
            ActivityState::InvincibleAfterStun,
            ActivityState::Stunned,
            ActivityState::Creating,
            ActivityState::Normal
        ]
        .map(ActivityState::state_id),
        [3, 2, 0, 0]
    );
    let mut game = recorded_game();
    game.set_activity_state(0, ActivityState::Creating, None, T0);
    // A newer state cancels the pending reset of the older one.
    game.set_activity_state(0, ActivityState::Stunned, None, T0 + 100);
    game.advance(T0 + 1300);
    assert_eq!(
        game.player(1).unwrap().attrs.activity_state,
        ActivityState::Stunned
    );
    let a = &game.player(1).unwrap().attrs;
    assert_eq!(a.activity_timer(T0 + 100), (15_000 / 300) * 5);
    assert_eq!(a.activity_timer(T0 + 3_100), (12_000 / 300) * 5);
}

#[test]
fn coordinates_convert_like_java() {
    assert_eq!(to_game_coordinate(80000), 25);
    assert_eq!(to_game_coordinate(80000 + 3199), 25);
    assert_eq!(to_game_coordinate(-3199), 0);
    assert_eq!(to_world_coordinate(42), 134400);
    assert_eq!(maps::raw_heightmap(4), maps::raw_heightmap(5));
    assert_ne!(maps::raw_heightmap(1), maps::raw_heightmap(4));
}

/// `scenarios/snowstorm-game.toml` relies on this: from any spawn of arena 4, Java's greedy
/// walk reaches a's tile (23,24) and b's (27,24) within 24 turns.
#[test]
fn every_spawn_of_arena_4_reaches_the_scenario_tiles() {
    let maps = SnowStormMaps::embedded();
    let map = maps.get(4).unwrap();
    let mut spawns = Vec::new();
    for cl in map.spawn_clusters() {
        for p in circle(Position::new(cl.x, cl.y), cl.radius) {
            if map.tile(p).is_some_and(|t| t.is_walkable()) && !spawns.contains(&p) {
                spawns.push(p);
            }
        }
    }
    assert!(spawns.len() > 1000);
    let mut game = SnowStormGame::new(4, 2, 1, &maps);
    game.add_player(bot(1, 0));
    game.initialise();
    game.assign_spawn_points(&mut |_| 0, T0);
    game.set_in_room(1, true);
    game.start_external_turns();
    for goal in [Position::new(23, 24), Position::new(27, 24)] {
        let mut worst = 0;
        for &spawn in &spawns {
            let a = &mut game.player_mut(1).unwrap().attrs;
            a.current_position = spawn;
            a.walking = spawn != goal;
            a.walk_goal = Some(goal);
            a.goal_world_coordinates = Some([goal.x * 3200, goal.y * 3200]);
            let mut turns = 0;
            while game.player(1).unwrap().attrs.walking && turns < 30 {
                game.tick(T0 + turns * 300);
                turns += 1;
            }
            if spawn == Position::new(0, 22) {
                // An island of one tile (1 spawn in 1241): the walk stops at once. The scenario
                // fails in such a run, on Java too.
                assert_ne!(game.player(1).unwrap().attrs.current_position, goal);
                continue;
            }
            assert_eq!(
                game.player(1).unwrap().attrs.current_position,
                goal,
                "from {spawn:?}"
            );
            worst = worst.max(turns);
        }
        assert!(worst <= 24, "{goal:?}: {worst} turns");
    }
}
