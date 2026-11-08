//! Unit tests of BattleBall: the colouring, sealing and filling of tiles, the points, and the
//! power ups (random in Java, so the traces play without them).

use std::sync::Arc;

use super::*;
use crate::game::games::game::{Game, GameKind};
use crate::game::games::player::{GamePlayer, PlayerDetails};
use crate::game::games::{GameType, arena_model};
use crate::session::Session;

fn world() -> (tempfile::TempDir, Arc<World>) {
    let dir = tempfile::tempdir().unwrap();
    let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
    (dir, Arc::new(World::new(store).unwrap()))
}

/// A 5x5 flat arena where every tile is a game tile, with player 1 (serial 1, user 1) in team 0
/// and player 2 (serial 2, user 2) in team 1.
fn game(world: &World, map_id: i32, power_ups: Vec<i32>) -> Game {
    let model = Arc::new(arena_model(
        "bb_arena_t",
        "00000|00000|00000|00000|00000|",
        9,
        9,
    ));
    let mut game = Game::new(
        world,
        1,
        1,
        map_id,
        GameType::BattleBall,
        "test".into(),
        2,
        (1, "one".into()),
        GameKind::BattleBall(Box::new(BattleBall::new(power_ups))),
    );
    for (serial, team) in [(1u64, 0), (2, 1)] {
        let mut gp = GamePlayer::new(
            serial,
            Session::detached(),
            PlayerDetails {
                user_id: serial as u32,
                name: format!("p{serial}"),
                ..Default::default()
            },
        );
        gp.team_id = team;
        gp.in_game = true;
        gp.game_id = 1;
        gp.object_id = serial as i32;
        game.players.push(gp);
        game.teams[team as usize].players.push(serial);
    }
    let map = TileMap::parse(map_id, "11111|11111|11111|11111|11111|");
    bb_of_mut(&mut game).build_map(&model, Some(&map));
    game.model = Some(model);
    game
}

fn tile(game: &Game, x: i32, y: i32) -> &Tile {
    let bb = bb_of(game);
    &bb.tiles[bb.tile_index(x, y).unwrap()]
}

fn step(game: &mut Game, serial: u64, x: i32, y: i32) -> (Vec<usize>, Vec<usize>) {
    let i = bb_of(game).tile_index(x, y).unwrap();
    let (mut updates, mut fills) = (Vec::new(), Vec::new());
    change_state(game, i, serial, &mut updates, &mut fills);
    (updates, fills)
}

#[test]
fn tile_maps_drop_the_trailing_row_and_fix_sky_peak() {
    let map = TileMap::parse(3, "011|110|");
    assert_eq!((map.size_x, map.size_y), (3, 2));
    assert!(!map.is_game_tile(0, 0));
    assert!(map.is_game_tile(1, 0));
    assert!(!map.is_game_tile(2, 1));
    assert!(!map.is_game_tile(5, 5));
    let rows = vec!["0".repeat(28); 28].join("|");
    let sky_peak = TileMap::parse(1, &rows);
    assert!(sky_peak.is_game_tile(24, 17) && sky_peak.is_game_tile(24, 18));
    assert!(!sky_peak.is_game_tile(24, 16));
}

#[test]
fn tiles_take_four_hits_on_barebones_classic_and_three_elsewhere() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    step(&mut g, 1, 1, 1);
    assert_eq!(
        (tile(&g, 1, 1).colour, tile(&g, 1, 1).state),
        (0, state::CLICKED)
    );
    step(&mut g, 1, 1, 1);
    assert_eq!(tile(&g, 1, 1).state, state::PRESSED);
    assert_eq!(bb_of(&g).player_score(1), 6 + 10);

    let mut g = game(&world, 5, Vec::new());
    step(&mut g, 1, 1, 1);
    assert_eq!(
        (tile(&g, 1, 1).colour, tile(&g, 1, 1).state),
        (0, state::TOUCHED)
    );
    assert_eq!(bb_of(&g).team_points(0), 2);
}

#[test]
fn stealing_a_tile_is_worth_two_more_and_drops_the_other_teams_points() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    step(&mut g, 1, 2, 2);
    step(&mut g, 2, 2, 2);
    assert_eq!(
        (tile(&g, 2, 2).colour, tile(&g, 2, 2).state),
        (1, state::CLICKED)
    );
    assert_eq!(bb_of(&g).team_points(0), 0);
    assert_eq!(bb_of(&g).team_points(1), 6 + 2);
    assert_eq!(bb_of(&g).player_score(2), 8);
}

#[test]
fn sealing_fills_what_the_team_closes_off() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    // Seal the four tiles around 2,2 (each: clicked, pressed, sealed); the last one closes it
    // off (the fill goes through edges, not corners).
    for (x, y) in [(2, 1), (1, 2), (3, 2)] {
        for _ in 0..3 {
            step(&mut g, 1, x, y);
        }
        assert_eq!(tile(&g, x, y).state, state::SEALED);
    }
    assert_eq!(tile(&g, 2, 2).state, state::DEFAULT);
    step(&mut g, 1, 2, 3);
    step(&mut g, 1, 2, 3);
    let (updates, fills) = step(&mut g, 1, 2, 3);
    assert_eq!(updates.len(), 1);
    let centre = bb_of(&g).tile_index(2, 2).unwrap();
    assert_eq!(fills, vec![centre]);
    assert_eq!(
        (tile(&g, 2, 2).colour, tile(&g, 2, 2).state),
        (0, state::SEALED)
    );
    // The outside reaches the edge of the map: not filled.
    assert_eq!(tile(&g, 0, 0).state, state::DEFAULT);
    // A sealed tile gives nothing more.
    assert_eq!(step(&mut g, 2, 2, 3), (vec![], vec![]));
    assert_eq!(bb_of(&g).player_score(1), 4 * (6 + 10 + 14));
}

#[test]
fn the_game_goes_on_until_every_tile_is_sealed() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    assert!(bb_of(&g).can_timer_continue());
    for t in &mut bb_of_mut(&mut g).tiles {
        t.state = state::SEALED;
    }
    assert!(!bb_of(&g).can_timer_continue());
}

#[test]
fn undoing_a_tile_clears_it() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    step(&mut g, 1, 0, 0);
    let i = bb_of(&g).tile_index(0, 0).unwrap();
    assert!(undo_tile(&mut bb_of_mut(&mut g).tiles[i]));
    assert_eq!(
        (tile(&g, 0, 0).colour, tile(&g, 0, 0).state),
        (colour::DEFAULT, state::DEFAULT)
    );
    assert!(tile(&g, 0, 0).points.is_empty());
    assert!(!undo_tile(&mut bb_of_mut(&mut g).tiles[i]));
}

#[test]
fn power_ups_bring_the_question_mark_and_are_picked_up() {
    assert_eq!(BattleBall::new(vec![1]).allowed_power_ups, vec![1]);
    assert_eq!(BattleBall::new(vec![1, 7]).allowed_power_ups, vec![1, 7, 9]);
    let (_dir, world) = world();
    let mut g = game(&world, 1, vec![9]);
    let i = bb_of(&g).tile_index(4, 4).unwrap();
    let mut power = PowerUp::new(7, bb_of(&g), i);
    assert!((20..=30).contains(&power.time_to_despawn));
    power.power_type = PowerType::QuestionMark;
    let bb = bb_of_mut(&mut g);
    bb.active_powers.push(power);
    bb.objects.push(Object::Power(7));
    let mut tick = Tick::default();
    powerups::check_power_up(&mut g, i, 1, &mut tick);
    let bb = bb_of(&g);
    assert!(bb.active_powers.is_empty() && bb.objects.is_empty());
    let held = &bb.stored_of(1).unwrap()[0];
    // A question mark becomes another allowed power up, a bomb when there is none.
    assert_eq!(held.power_type, PowerType::Bomb);
    assert_eq!((held.time_to_despawn, held.holder), (15, Some(1)));
    assert_eq!(
        tick.events,
        vec![Event::Acquire {
            serial: 1,
            power: 7,
            ty: PowerType::Bomb as i32
        }]
    );
    assert_eq!(tick.objects, vec![Update::Power(7)]);
}

#[test]
fn power_ups_expire() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, vec![1, 2]);
    let i = bb_of(&g).tile_index(0, 0).unwrap();
    let mut power = PowerUp::new(3, bb_of(&g), i);
    power.time_to_despawn = 2;
    let bb = bb_of_mut(&mut g);
    bb.active_powers.push(power);
    bb.objects.push(Object::Power(3));
    bb.check_expire_power(1);
    assert_eq!(bb.active_powers.len(), 1);
    bb.check_expire_power(1);
    assert!(bb.active_powers.is_empty() && bb.objects.is_empty());
    assert_eq!(bb.events_queue, vec![Event::Despawn(3)]);
}

#[test]
fn the_spring_seals_and_the_drill_cleans() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    g.player_mut(1).unwrap().player_state = Some(PlayerState::HighJumps);
    let i = bb_of(&g).tile_index(1, 0).unwrap();
    let (mut updates, mut fills) = (Vec::new(), Vec::new());
    assert!(powerups::has_used_power(
        &mut g,
        i,
        1,
        &mut updates,
        &mut fills
    ));
    assert_eq!(
        (tile(&g, 1, 0).colour, tile(&g, 1, 0).state),
        (0, state::SEALED)
    );
    assert_eq!(bb_of(&g).player_score(1), 14);
    g.player_mut(2).unwrap().player_state = Some(PlayerState::CleaningTiles);
    assert!(powerups::has_used_power(
        &mut g,
        i,
        2,
        &mut updates,
        &mut fills
    ));
    assert_eq!(tile(&g, 1, 0).state, state::DEFAULT);
    assert_eq!(updates, vec![i, i]);
    g.player_mut(2).unwrap().player_state = Some(PlayerState::Normal);
    assert!(!powerups::has_used_power(
        &mut g,
        i,
        2,
        &mut updates,
        &mut fills
    ));
}

#[test]
fn harlequin_players_colour_for_the_other_team() {
    let (_dir, world) = world();
    let mut g = game(&world, 1, Vec::new());
    g.player_mut(2).unwrap().harlequin = Some(1);
    step(&mut g, 2, 4, 4);
    assert_eq!(tile(&g, 4, 4).colour, 0);
    // The points go to the harlequin's player and team.
    assert_eq!(bb_of(&g).player_score(1), 6);
    assert_eq!(bb_of(&g).player_score(2), 0);
}
