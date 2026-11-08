//! Java: `GamePoker`, `GamePokerOptions` and `GamePokerCombination`: five-card draw for up to
//! four players. A round starts with the first `STARTOVER`; each player may change cards once,
//! and the round ends when everyone in it changed.
//!
//! The options (`poker.*` settings) can charge an entry price and reward the winners with
//! credits, rares and tickets. Java does those balance changes inside the command; here they
//! wait until the room's lock is released (a player may not be locked inside a room), so
//! `CREDIT_BALANCE`, `TICKET_BALANCE` and a rare's hand come after the command's other messages.

use std::sync::Arc;

use rand::RngExt;

use super::{Aborted, Command, GameInstance, GamePlayer, GameState, java_parse_int};
use crate::game::room::Room;
use crate::messages::outgoing::rooms_games::item_msg_raw;
use crate::world::World;

const JOKER: &str = "joker";
const FACES: [&str; 13] = [
    "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "1",
];
const SUITS: [&str; 4] = ["h", "d", "c", "s"];

/// Java: `GamePokerCombination`, best first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combination {
    FiveOfAKind,
    RoyalFlush,
    StraightFlush,
    FourOfAKind,
    FullHouse,
    Flush,
    Straight,
    ThreeOfAKind,
    TwoPair,
    OnePair,
    HighCard,
}

impl Combination {
    pub fn name(self) -> &'static str {
        match self {
            Self::FiveOfAKind => "a Five of a kind",
            Self::RoyalFlush => "a Royal flush",
            Self::StraightFlush => "a Straight flush",
            Self::FourOfAKind => "a Four of a kind",
            Self::FullHouse => "a Full House",
            Self::Flush => "a Flush",
            Self::Straight => "a Straight",
            Self::ThreeOfAKind => "a Three of a kind",
            Self::TwoPair => "two pairs",
            Self::OnePair => "one pair",
            Self::HighCard => "an high card",
        }
    }

    /// Java: `getCombinationRank`: `|ordinal - 10|`.
    fn rank(self) -> i32 {
        (self as i32 - 10).abs()
    }

    /// Java: `GamePokerCombination.getCombination`; `None` for a hand Java does not know.
    pub fn of(hand: &[String]) -> Option<Self> {
        let cards: Vec<&String> = hand.iter().filter(|c| *c != JOKER).collect();
        let jokers = (hand.len() - cards.len()) as i32;
        let suits: Vec<i32> = cards
            .iter()
            .map(|c| index_of(&SUITS, c.get(..1).unwrap_or("")))
            .collect();
        let faces: Vec<i32> = cards
            .iter()
            .map(|c| index_of(&FACES, c.get(1..).unwrap_or("")))
            .collect();
        if cards
            .iter()
            .any(|c| cards.iter().filter(|o| o == &c).count() > 1)
            || faces.contains(&-1)
            || suits.contains(&-1)
        {
            return None;
        }
        let flush = suits.iter().all(|s| Some(s) == suits.first());
        let mut groups: Vec<i32> = (0..13)
            .map(|f| faces.iter().filter(|&&x| x == f).count() as i32)
            .collect();
        groups.sort_by(|a, b| b.cmp(a));
        let shifted: Vec<i32> = faces.iter().map(|f| (f + 1) % 13).collect();
        // Java: Collections.max of an empty list throws (a hand of jokers only).
        let spread = |v: &[i32]| Some(v.iter().max()? - v.iter().min()?);
        let distance = spread(&faces)?.min(spread(&shifted)?);
        let straight = groups[0] == 1 && distance < 5;
        let royal = straight && flush && !faces.iter().any(|&f| f < 8);
        groups[0] += jokers;
        Some(if groups[0] == 5 {
            Self::FiveOfAKind
        } else if royal {
            Self::RoyalFlush
        } else if straight && flush {
            Self::StraightFlush
        } else if groups[0] == 4 {
            Self::FourOfAKind
        } else if groups[0] == 3 && groups[1] == 2 {
            Self::FullHouse
        } else if flush {
            Self::Flush
        } else if straight {
            Self::Straight
        } else if groups[0] == 3 {
            Self::ThreeOfAKind
        } else if groups[0] == 2 && groups[1] == 2 {
            Self::TwoPair
        } else if groups[0] == 2 {
            Self::OnePair
        } else {
            Self::HighCard
        })
    }
}

fn index_of(list: &[&str], s: &str) -> i32 {
    list.iter().position(|x| *x == s).map_or(-1, |i| i as i32)
}

/// Java: `GamePokerCombination.getDeck`: two jokers, then each suit 2 to king and the ace.
pub fn deck() -> Vec<String> {
    let mut deck = vec![JOKER.to_string(), JOKER.to_string()];
    for suit in SUITS {
        for face in FACES {
            deck.push(format!("{suit}{face}"));
        }
    }
    deck
}

/// Java: `GamePokerOptions`, read when a game starts.
#[derive(Clone, Debug)]
pub struct PokerOptions {
    entry_price: i32,
    min_players: usize,
    redistribute_entry: bool,
    redistribute_on_tie: bool,
    credits_bonus: i32,
    credits_bonus_on_tie: bool,
    rares_reward: Vec<String>,
    rares_quantity: i32,
    rares_on_tie: bool,
    tickets_reward: i32,
    tickets_on_tie: bool,
    announce_winner: bool,
    announce_rewards: bool,
}

impl Default for PokerOptions {
    /// What Java's defaults give (also used when the world is gone).
    fn default() -> Self {
        Self {
            entry_price: 0,
            min_players: 2,
            redistribute_entry: true,
            redistribute_on_tie: true,
            credits_bonus: 0,
            credits_bonus_on_tie: false,
            rares_reward: vec![String::new()],
            rares_quantity: 0,
            rares_on_tie: false,
            tickets_reward: 0,
            tickets_on_tie: false,
            announce_winner: true,
            announce_rewards: false,
        }
    }
}

impl PokerOptions {
    /// Java: the `GamePokerOptions` constructor for the room `room_id`. A value is only used in
    /// the rooms its `<name>.only.in.rooms` lists (a substring test of the room id, as in Java;
    /// an empty list means every room), else the default.
    pub fn load(world: &World, room_id: u32) -> Self {
        let settings = world.settings.read();
        let active = |name: &str| {
            let key = format!("{name}.only.in.rooms");
            let rooms = if settings.contains(&key) {
                settings.string(&key)
            } else {
                ""
            };
            rooms.trim().is_empty() || rooms.contains(&room_id.to_string())
        };
        // Java: GameConfiguration.getInteger is Integer.parseInt(getOrDefault(key, "0")).
        let int = |name: &str, def: i32| {
            let value = if settings.contains(name) {
                settings.string(name).parse().unwrap_or(0)
            } else {
                0
            };
            if active(name) { value } else { def }
        };
        // Java: getBoolean is "true" (any case), "1" or "yes" (any case).
        let boolean = |name: &str, def: bool| {
            let v = settings.string(name);
            let value = v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes");
            if active(name) { value } else { def }
        };
        let rares = {
            let name = "poker.reward.rares";
            let value = if settings.contains(name) {
                settings.string(name).to_string()
            } else {
                String::new()
            };
            let value = if active(name) { value } else { String::new() };
            java_split_comma(&value)
        };
        Self {
            entry_price: int("poker.entry.price", 0),
            min_players: int("poker.reward.min.player", 2).max(0) as usize,
            redistribute_entry: boolean("poker.entry.price.redistribute", true),
            redistribute_on_tie: boolean("poker.entry.price.redistribute.on.tie", true),
            credits_bonus: int("poker.reward.credits.bonus", 0),
            credits_bonus_on_tie: boolean("poker.reward.credits.bonus.on.tie", false),
            rares_reward: rares,
            rares_quantity: int("poker.reward.rares.quantity", 0),
            rares_on_tie: boolean("poker.reward.rares.on.tie", false),
            tickets_reward: int("poker.reward.tickets", 0),
            tickets_on_tie: boolean("poker.reward.tickets.on.tie", false),
            announce_winner: boolean("poker.announce.winner", true),
            announce_rewards: boolean("poker.announce.rewards", false),
        }
    }
}

/// Java: `String.split(",")` (an empty string gives one empty entry).
fn java_split_comma(s: &str) -> Vec<String> {
    super::java_split(s, ',')
}

/// The game's state (Java: `GamePoker`'s fields).
pub struct Poker {
    /// Java: `playersInGame`: who played at least one round (left by `disconnectPlayer`).
    in_game: Vec<GamePlayer>,
    /// Java: `playersPlaying`: who is in this round (a `STARTOVER` twice adds a player twice).
    playing: Vec<GamePlayer>,
    deck: Vec<String>,
    /// Java: `playerChanges` (a `HashMap`; here in the order of the changes).
    changes: Vec<(GamePlayer, Vec<String>)>,
    /// Java: `playerCards` (a `HashMap`; here in the order cards were dealt).
    cards: Vec<(GamePlayer, Vec<String>)>,
    options: PokerOptions,
    /// Java: `GamePokerOptions.entryPaid`.
    entry_paid: i32,
    /// Java: `GamePokerOptions.rewardMessages`.
    reward_messages: Vec<String>,
}

fn get<'a, T>(list: &'a [(GamePlayer, T)], p: &GamePlayer) -> Option<&'a T> {
    list.iter().find(|(q, _)| q == p).map(|(_, v)| v)
}

fn put<T>(list: &mut Vec<(GamePlayer, T)>, p: &GamePlayer, value: T) {
    match list.iter_mut().find(|(q, _)| q == p) {
        Some(entry) => entry.1 = value,
        None => list.push((p.clone(), value)),
    }
}

fn remove<T>(list: &mut Vec<(GamePlayer, T)>, p: &GamePlayer) {
    list.retain(|(q, _)| q != p);
}

impl Poker {
    /// Java: `gameStart`.
    pub fn new(options: PokerOptions) -> Self {
        Self {
            in_game: Vec::new(),
            playing: Vec::new(),
            deck: Vec::new(),
            changes: Vec::new(),
            cards: Vec::new(),
            options,
            entry_paid: 0,
            reward_messages: Vec::new(),
        }
    }

    /// Java: `gameStop`.
    pub fn stop(&mut self) {
        self.clear_round();
        self.in_game.clear();
    }

    /// Java: `clearRound`.
    fn clear_round(&mut self) {
        self.entry_paid = 0;
        self.reward_messages.clear();
        self.deck.clear();
        self.cards.clear();
        self.changes.clear();
        self.playing.clear();
    }

    fn is_round_running(&self) -> bool {
        !self.deck.is_empty()
    }

    fn has_everyone_changed(&self) -> bool {
        self.playing
            .iter()
            .filter(|p| get(&self.changes, p).is_some())
            .count()
            == self.playing.len()
    }

    /// Java: `deck.remove(random index)`, which throws on an empty deck.
    fn draw(&mut self) -> Result<String, Aborted> {
        if self.deck.is_empty() {
            return Err(Aborted);
        }
        let i = rand::rng().random_range(0..self.deck.len());
        Ok(self.deck.remove(i))
    }

    /// Java: `getDisconnectedPlayers().forEach(disconnectPlayer)`: players of the game who left
    /// the table.
    fn disconnect_gone(&mut self, game: &GameInstance) {
        let gone: Vec<GamePlayer> = self
            .in_game
            .iter()
            .filter(|p| !game.players.contains(p))
            .cloned()
            .collect();
        for p in gone {
            self.disconnect(game, &p);
        }
    }

    /// Java: `disconnectPlayer`.
    fn disconnect(&mut self, game: &GameInstance, player: &GamePlayer) {
        if !self.in_game.contains(player) {
            return;
        }
        self.in_game.retain(|p| p != player);
        self.playing.retain(|p| p != player);
        remove(&mut self.changes, player);
        remove(&mut self.cards, player);
        let msg = item_msg_raw(&format!("{}\rOPPONENT_LOGOUT\r{}", game.id(), player.name));
        for p in &self.in_game {
            p.send(msg.clone());
        }
    }

    fn change_count(&self, p: &GamePlayer) -> usize {
        get(&self.changes, p).map_or(0, Vec::len)
    }

    /// Java: `broadcastOpponents(player)`.
    fn send_opponents_to(&self, game: &GameInstance, player: &GamePlayer) {
        let mut data = vec![format!("{} {}", player.name, self.change_count(player))];
        for p in &self.in_game {
            if p != player {
                data.push(format!("{} {}", p.name, self.change_count(p)));
            }
        }
        player.send(game.item_msg(&["OPPONENTS", &data.join("\r")]));
    }

    /// Java: `broadcastOpponents()`.
    fn broadcast_opponents(&self, game: &GameInstance) {
        for p in &self.in_game {
            self.send_opponents_to(game, p);
        }
    }

    /// Java: `formatPlayerChangedUpdate` (only called for players who changed).
    fn changed_update(&self, game: &GameInstance, player: &GamePlayer) -> String {
        let Some(changed) = get(&self.changes, player) else {
            return String::new();
        };
        format!(
            "{}\rCHANGED\r{}/{}",
            game.id(),
            player.name,
            changed.join(" ")
        )
    }

    /// Java: `formatRevealCardsUpdate`.
    fn reveal_update(&self, player: &GamePlayer) -> Result<String, Aborted> {
        let changed = get(&self.changes, player);
        // Java: String.join over playerCards.get(player) throws without a hand.
        let cards = get(&self.cards, player).ok_or(Aborted)?;
        Ok(format!(
            "{},{}/{}",
            if changed.is_some() { "1" } else { "0" },
            changed.map_or(0, Vec::len),
            cards.join("/")
        ))
    }

    /// Java: `endRound`. Java's reward code can throw (see [`Poker::on_finish`]); then the cards
    /// are never revealed and the round is not cleared.
    fn end_round(&mut self, room: &mut Room, game: &GameInstance) -> Result<(), Aborted> {
        self.on_finish(room)?;
        let mut updates = Vec::new();
        for p in &self.playing {
            updates.push(format!("{}/{}", p.name, self.reveal_update(p)?));
        }
        let msg = game.item_msg(&["REVEALCARDS", &updates.join("\r")]);
        for p in &self.playing {
            p.send(msg.clone());
        }
        self.clear_round();
        Ok(())
    }

    /// Java: `GamePokerOptions.onEntry`: the entry price.
    fn on_entry(&mut self, room: &mut Room, player: &GamePlayer) {
        let price = self.options.entry_price;
        if price <= 0 {
            return;
        }
        self.entry_paid += price;
        let session = player.session.clone();
        room.defer(move |world| {
            let mut p = session.player.lock();
            if let Some(details) = p.details.as_mut() {
                crate::game::currency::decrease_credits(&world.store, details, price);
                session.send(crate::messages::outgoing::user::credit_balance(
                    details.credits,
                ));
            }
        });
    }

    /// Java: `GamePokerOptions.getWinners`.
    fn winners(&self) -> Result<(Combination, Vec<GamePlayer>), Aborted> {
        let mut best: Option<Combination> = None;
        let mut winners = Vec::new();
        for (p, hand) in &self.cards {
            // Java: getCombinationRank of a null combination throws.
            let combination = Combination::of(hand).ok_or(Aborted)?;
            match best {
                None => {
                    best = Some(combination);
                    winners.push(p.clone());
                }
                Some(b) if combination.rank() > b.rank() => {
                    best = Some(combination);
                    winners.clear();
                    winners.push(p.clone());
                }
                Some(b) if combination.rank() == b.rank() => winners.push(p.clone()),
                _ => {}
            }
        }
        Ok((best.ok_or(Aborted)?, winners))
    }

    /// Java: `GamePokerOptions.onFinish`.
    fn on_finish(&mut self, room: &mut Room) -> Result<(), Aborted> {
        let players: Vec<GamePlayer> = self.cards.iter().map(|(p, _)| p.clone()).collect();
        let (combination, winners) = self.winners()?;
        if self.options.announce_winner {
            let text = if winners.len() == 1 {
                format!(
                    "{} wins this round with {}. Congratulations!",
                    winners[0].name,
                    combination.name()
                )
            } else {
                let names: Vec<&str> = winners.iter().map(|p| p.name.as_str()).collect();
                format!(
                    "It's a tie! {} all have {}. Congratulations!",
                    names.join(", "),
                    combination.name()
                )
            };
            room.game_chat(&players, &text);
        }
        if players.len() < self.options.min_players || !self.can_reward(&winners) {
            return Ok(());
        }
        self.reward_credits(room, &winners);
        self.reward_rares(room, &winners)?;
        self.reward_tickets(room, &winners);
        if self.options.announce_rewards {
            for msg in &self.reward_messages {
                room.game_chat(&players, msg);
            }
        }
        Ok(())
    }

    /// Java: `canReward` (an empty `poker.reward.rares` still splits into one entry).
    fn can_reward(&self, winners: &[GamePlayer]) -> bool {
        let o = &self.options;
        let tie = winners.len() > 1;
        (o.redistribute_entry && self.entry_paid > 0 && (!tie || o.redistribute_on_tie))
            || (!o.rares_reward.is_empty() && (!tie || o.rares_on_tie))
            || (o.tickets_reward > 0 && (!tie || o.tickets_on_tie))
            || (o.credits_bonus > 0 && (!tie || o.credits_bonus_on_tie))
    }

    /// Java: `rewardCredits`.
    fn reward_credits(&mut self, room: &mut Room, winners: &[GamePlayer]) {
        let o = &self.options;
        let mut credits = 0;
        if o.redistribute_entry && (winners.len() == 1 || o.redistribute_on_tie) {
            credits += self.entry_paid;
        }
        if o.credits_bonus > 0 && (winners.len() == 1 || o.credits_bonus_on_tie) {
            credits += o.credits_bonus;
        }
        if credits == 0 {
            return;
        }
        let each = credits / winners.len() as i32;
        for w in winners {
            let session = w.session.clone();
            room.defer(move |world| {
                let mut p = session.player.lock();
                let Some(details) = p.details.as_mut() else {
                    return;
                };
                match world.store.update_user(details.id, |u| u.credits += each) {
                    Ok(Some(stored)) => details.credits = stored.credits,
                    Ok(None) => {}
                    Err(e) => tracing::error!(user = details.id, "poker credits: {e}"),
                }
                session.send(crate::messages::outgoing::user::credit_balance(
                    details.credits,
                ));
            });
        }
        self.reward_messages.push(if winners.len() > 1 {
            format!("Each winner received {each} credits!")
        } else {
            format!("{} received {each} credits!", winners[0].name)
        });
    }

    /// Java: `rewardRares`. A sale code with no catalogue item (as the empty default gives)
    /// makes Java throw here.
    fn reward_rares(&mut self, room: &mut Room, winners: &[GamePlayer]) -> Result<(), Aborted> {
        let o = &self.options;
        if o.rares_reward.is_empty()
            || o.rares_quantity == 0
            || (winners.len() > 1 && !o.rares_on_tie)
        {
            return Ok(());
        }
        let world = room.world().ok_or(Aborted)?;
        // Java: new CatalogueItem[negative] throws.
        let quantity = usize::try_from(o.rares_quantity).map_err(|_| Aborted)?;
        let mut rares = Vec::new();
        let mut names = Vec::new();
        for _ in 0..quantity {
            let code = &o.rares_reward[rand::rng().random_range(0..o.rares_reward.len())];
            let item = world.catalogue.item_by_sale_code(code).ok_or(Aborted)?;
            let def = item.definition.as_ref().ok_or(Aborted)?;
            names.push(def.record.name.clone());
            rares.push(item.id);
        }
        for w in winners {
            let session = w.session.clone();
            let rares = rares.clone();
            room.defer(move |world| give_rares(world, &session, &rares));
        }
        let list = names.join(", ");
        self.reward_messages.push(if winners.len() > 1 {
            format!("Each winner received {list} !")
        } else {
            format!("{} received {list} !", winners[0].name)
        });
        Ok(())
    }

    /// Java: `rewardTickets`.
    fn reward_tickets(&mut self, room: &mut Room, winners: &[GamePlayer]) {
        let o = &self.options;
        if o.tickets_reward <= 0 || (winners.len() > 1 && !o.tickets_on_tie) {
            return;
        }
        let amount = o.tickets_reward;
        for w in winners {
            let session = w.session.clone();
            room.defer(move |world| {
                let mut p = session.player.lock();
                let Some(details) = p.details.as_mut() else {
                    return;
                };
                match world.store.update_user(details.id, |u| u.tickets += amount) {
                    Ok(Some(stored)) => details.tickets = stored.tickets,
                    Ok(None) => {}
                    Err(e) => tracing::error!(user = details.id, "poker tickets: {e}"),
                }
                session.send(crate::messages::outgoing::rooms_pool::ticket_balance(
                    details.tickets,
                ));
            });
        }
        self.reward_messages.push(if winners.len() > 1 {
            format!("Each winner received {amount} tickets!")
        } else {
            format!("{} received {amount} tickets!", winners[0].name)
        });
    }
}

/// Java: `catalogue.purchase(winner, rare, ...)` for each rare (one transaction here), then
/// `getView("new")`.
fn give_rares(world: &World, session: &Arc<crate::session::Session>, rares: &[i32]) {
    let mut p = session.player.lock();
    let Some(details) = p.details.clone() else {
        return;
    };
    let result = (|| -> anyhow::Result<Vec<crate::game::catalogue::Purchase>> {
        let mut w = world.store.write()?;
        let mut bought = Vec::new();
        for id in rares {
            if let Some(item) = world.catalogue.item(*id) {
                bought.push(world.catalogue.purchase_in(
                    &mut w,
                    &details,
                    item,
                    "",
                    crate::game::now(),
                )?);
            }
        }
        w.commit()?;
        Ok(bought)
    })();
    match result {
        Ok(bought) => {
            for purchase in bought {
                purchase.apply(&world.items, session, &mut p);
            }
            let hand = p.inventory.get_view(&world.store, "new");
            session.send(crate::messages::outgoing::inventory::inventory(&hand));
        }
        Err(e) => tracing::error!(user = details.id, "poker rares: {e}"),
    }
}

/// Java: `GamePoker.handleCommand`.
pub fn handle(
    room: &mut Room,
    game: &mut GameInstance,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    let player = cmd.player;
    if command == "CLOSE" {
        room.leave_game(game, &player.session);
    }
    let GameState::Poker(mut state) = std::mem::replace(&mut game.state, GameState::Idle) else {
        return Ok(());
    };
    let result = command_on(room, game, &mut state, player, command, args);
    game.state = GameState::Poker(state);
    result
}

fn command_on(
    room: &mut Room,
    game: &GameInstance,
    k: &mut Poker,
    player: &GamePlayer,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    match command {
        "CLOSE" => {
            k.disconnect_gone(game);
            if k.is_round_running() && k.has_everyone_changed() {
                k.end_round(room, game)?;
            }
        }
        "OPEN" => {
            k.disconnect_gone(game);
            k.send_opponents_to(game, player);
        }
        "STARTOVER" => {
            if !k.is_round_running() {
                k.clear_round();
                k.deck.extend(deck());
            }
            // Java: addPlayerToGame.
            if !k.in_game.contains(player) {
                k.in_game.push(player.clone());
                k.broadcast_opponents(game);
            }
            // Java: addPlayerToRound.
            k.playing.push(player.clone());
            for (p, _) in &k.changes {
                player.send(item_msg_raw(&k.changed_update(game, p)));
            }
            k.on_entry(room, player);
            // Java: giveCardsToPlayer.
            let mut cards = Vec::new();
            for _ in 0..5 {
                cards.push(k.draw()?);
            }
            put(&mut k.cards, player, cards);
            remove(&mut k.changes, player);
            let reveal = k.reveal_update(player)?;
            player.send(game.item_msg(&[&format!("YOURCARDS 0/{reveal}")]));
            k.broadcast_opponents(game);
        }
        "CHANGE" => {
            k.disconnect_gone(game);
            change_cards(k, game, player, args)?;
            k.broadcast_opponents(game);
            if k.is_round_running() && k.has_everyone_changed() {
                k.end_round(room, game)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Java: `changeCards`.
fn change_cards(
    k: &mut Poker,
    game: &GameInstance,
    player: &GamePlayer,
    indexes: &[&str],
) -> Result<(), Aborted> {
    if get(&k.changes, player).is_some() {
        return Ok(());
    }
    let hand = get(&k.cards, player).cloned();
    let mut to_change = Vec::new();
    for s in indexes {
        if super::java_trim(s).is_empty() {
            continue;
        }
        let index = java_parse_int(s)?;
        // Java: playerHand.size() of a missing hand throws.
        let size = hand.as_ref().ok_or(Aborted)?.len() as i32;
        if index >= 0 && index < size {
            to_change.push(index);
        }
    }
    to_change.sort();
    let hand = hand.ok_or(Aborted)?;
    let mut changed_hand = Vec::with_capacity(hand.len());
    for card in &hand {
        // Java looks a card up by indexOf: the second joker counts as the first one.
        let index = hand.iter().position(|c| c == card).unwrap_or(0) as i32;
        if to_change.contains(&index) {
            changed_hand.push(k.draw()?);
        } else {
            changed_hand.push(card.clone());
        }
    }
    let changed: Vec<String> = to_change.iter().map(i32::to_string).collect();
    put(&mut k.cards, player, changed_hand);
    put(&mut k.changes, player, changed);
    let msg = item_msg_raw(&k.changed_update(game, player));
    for p in &k.playing {
        if p != player {
            p.send(msg.clone());
        }
    }
    let reveal = k.reveal_update(player)?;
    player.send(item_msg_raw(&format!(
        "{}\rYOURCARDS 0/{reveal}",
        game.id()
    )));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(cards: &[&str]) -> Vec<String> {
        cards.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn combinations() {
        use Combination::*;
        let cases: &[(&[&str], Combination)] = &[
            (&["h1", "h13", "h12", "h11", "h10"], RoyalFlush),
            (&["h9", "h13", "h12", "h11", "h10"], StraightFlush),
            (&["h2", "d2", "c2", "s2", "h10"], FourOfAKind),
            (&["h2", "d2", "c2", "s10", "h10"], FullHouse),
            (&["h2", "h5", "h7", "h9", "h11"], Flush),
            (&["h2", "d3", "c4", "s5", "h6"], Straight),
            (&["h1", "d2", "c3", "s4", "h5"], Straight),
            (&["h2", "d2", "c2", "s5", "h6"], ThreeOfAKind),
            (&["h2", "d2", "c5", "s5", "h6"], TwoPair),
            (&["h2", "d2", "c7", "s5", "h6"], OnePair),
            (&["h2", "d4", "c7", "s9", "h11"], HighCard),
            (&["joker", "joker", "c2", "s2", "h2"], FiveOfAKind),
            (&["joker", "d4", "c7", "s9", "h11"], OnePair),
        ];
        for (cards, expected) in cases {
            assert_eq!(Combination::of(&hand(cards)), Some(*expected), "{cards:?}");
        }
        assert_eq!(
            Combination::of(&hand(&["h2", "h2", "c7", "s9", "h11"])),
            None
        );
        assert_eq!(Combination::FiveOfAKind.rank(), 10);
        assert_eq!(Combination::HighCard.rank(), 0);
    }

    #[test]
    fn the_deck() {
        let d = deck();
        assert_eq!(d.len(), 54);
        assert_eq!(&d[..4], &["joker", "joker", "h2", "h3"]);
        assert_eq!(d[14], "h1");
    }
}
