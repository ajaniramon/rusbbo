//! Messenger tables (Java: `dao/mysql/MessengerDao.java`): friends, friend requests, offline
//! messages and console categories.
//!
//! Layout, derived from the `WHERE` clauses of `MessengerDao`:
//! - `messenger_friends`: `(owner, friend) -> category_id`. The Java table stores a row
//!   `(from_id = friend, to_id = owner)` for each direction of a friendship; here the key is
//!   `(owner, friend)` so one range scan over `owner` lists a user's friends.
//! - `messenger_requests`: `(to_id, from_id) -> ()`, listed by the receiver.
//! - `messenger_messages`: `id -> MessengerMessage`, plus `(receiver, id)` to find a receiver's mail.
//! - `messenger_categories`: `id -> MessengerCategory`, plus `(user_id, id)`.
//!
//! Message and category ids come from a counter that mimics MariaDB's AUTO_INCREMENT (the next id
//! is one past the highest ever handed out, and the importer copies the source's counter).

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, USERS_BY_NAME, Writer, decode, encode};

const FRIENDS: TableDefinition<(u32, u32), i32> = TableDefinition::new("messenger_friends");
const REQUESTS: TableDefinition<(u32, u32), ()> = TableDefinition::new("messenger_requests");
const MESSAGES: TableDefinition<u32, &[u8]> = TableDefinition::new("messenger_messages");
const MESSAGES_BY_RECEIVER: TableDefinition<(u32, u32), ()> =
    TableDefinition::new("messenger_messages_by_receiver");
const CATEGORIES: TableDefinition<u32, &[u8]> = TableDefinition::new("messenger_categories");
const CATEGORIES_BY_USER: TableDefinition<(u32, u32), ()> =
    TableDefinition::new("messenger_categories_by_user");
/// Next AUTO_INCREMENT value per table (`"messages"`, `"categories"`).
const SEQ: TableDefinition<&str, u32> = TableDefinition::new("messenger_seq");

/// Most rows the Java user search returns (`LIMIT 30`).
pub const SEARCH_LIMIT: usize = 30;

/// A row of `messenger_messages`. `date` is Unix seconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessengerMessage {
    pub id: u32,
    pub receiver_id: u32,
    pub sender_id: u32,
    pub unread: bool,
    pub body: String,
    pub date: i64,
}

/// A row of `messenger_categories`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessengerCategory {
    pub id: u32,
    pub user_id: u32,
    pub name: String,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(FRIENDS)?;
    tx.open_table(REQUESTS)?;
    tx.open_table(MESSAGES)?;
    tx.open_table(MESSAGES_BY_RECEIVER)?;
    tx.open_table(CATEGORIES)?;
    tx.open_table(CATEGORIES_BY_USER)?;
    tx.open_table(SEQ)?;
    Ok(())
}

impl Store {
    /// Friends of `user_id` with their category (Java: `getFriends`, before the join with `users`).
    pub fn messenger_friends(&self, user_id: u32) -> Result<Vec<(u32, i32)>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(FRIENDS)?;
        let mut out = Vec::new();
        for entry in table.range((user_id, 0)..=(user_id, u32::MAX))? {
            let (key, category) = entry?;
            out.push((key.value().1, category.value()));
        }
        Ok(out)
    }

    /// Ids of the users who asked `user_id` to be friends (Java: `getRequests`).
    pub fn messenger_requests(&self, user_id: u32) -> Result<Vec<u32>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(REQUESTS)?;
        let mut out = Vec::new();
        for entry in table.range((user_id, 0)..=(user_id, u32::MAX))? {
            out.push(entry?.0.value().1);
        }
        Ok(out)
    }

    /// Java: `requestExists`.
    pub fn messenger_request_exists(&self, from_id: u32, to_id: u32) -> Result<bool> {
        let tx = self.db.begin_read()?;
        Ok(tx.open_table(REQUESTS)?.get((to_id, from_id))?.is_some())
    }

    /// Java: `friendExists`: whether `friend_id` is in the list of `owner_id`.
    pub fn messenger_friend_exists(&self, owner_id: u32, friend_id: u32) -> Result<bool> {
        let tx = self.db.begin_read()?;
        Ok(tx
            .open_table(FRIENDS)?
            .get((owner_id, friend_id))?
            .is_some())
    }

    /// Java: `newRequest`. Ignores a request to oneself; one that already exists stays as it is.
    pub fn messenger_new_request(&self, from_id: u32, to_id: u32) -> Result<()> {
        if from_id == to_id {
            return Ok(());
        }
        let w = self.write()?;
        w.tx.open_table(REQUESTS)?.insert((to_id, from_id), ())?;
        w.commit()
    }

    /// Java: `removeRequest`: removes the request in both directions.
    pub fn messenger_remove_request(&self, from_id: u32, to_id: u32) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(REQUESTS)?;
            table.remove((to_id, from_id))?;
            table.remove((from_id, to_id))?;
        }
        w.commit()
    }

    /// Java: `removeAllRequests`.
    pub fn messenger_remove_all_requests(&self, to_id: u32) -> Result<()> {
        let w = self.write()?;
        w.tx.open_table(REQUESTS)?
            .retain_in((to_id, 0)..=(to_id, u32::MAX), |_, _| false)?;
        w.commit()
    }

    /// Puts `friend_id` in the list of `owner_id` (Java: `newFriend(owner, friend)`, which inserts
    /// the row `from_id = friend, to_id = owner`).
    pub fn messenger_add_friend(&self, owner_id: u32, friend_id: u32) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(FRIENDS)?;
            let exists = table.get((owner_id, friend_id))?.is_some();
            if !exists {
                table.insert((owner_id, friend_id), 0)?;
            }
        }
        w.commit()
    }

    /// Java `addFriend`'s three DAO calls in one transaction (one fsync instead of three): drops
    /// the request in both directions and lists each user in the other's friends.
    pub fn messenger_make_friends(&self, user_id: u32, buddy_id: u32) -> Result<()> {
        let w = self.write()?;
        {
            let mut requests = w.tx.open_table(REQUESTS)?;
            requests.remove((user_id, buddy_id))?;
            requests.remove((buddy_id, user_id))?;
            let mut friends = w.tx.open_table(FRIENDS)?;
            for (owner, friend) in [(user_id, buddy_id), (buddy_id, user_id)] {
                let exists = friends.get((owner, friend))?.is_some();
                if !exists {
                    friends.insert((owner, friend), 0)?;
                }
            }
        }
        w.commit()
    }

    /// Takes `friend_id` out of the list of `owner_id` (Java: `removeFriend`).
    pub fn messenger_remove_friend(&self, owner_id: u32, friend_id: u32) -> Result<()> {
        let w = self.write()?;
        w.tx.open_table(FRIENDS)?.remove((owner_id, friend_id))?;
        w.commit()
    }

    /// Unread messages of `user_id`, lowest id first (Java: `getUnreadMessages`).
    pub fn messenger_unread(&self, user_id: u32) -> Result<Vec<MessengerMessage>> {
        let tx = self.db.begin_read()?;
        let by_receiver = tx.open_table(MESSAGES_BY_RECEIVER)?;
        let messages = tx.open_table(MESSAGES)?;
        let mut out = Vec::new();
        for entry in by_receiver.range((user_id, 0)..=(user_id, u32::MAX))? {
            let id = entry?.0.value().1;
            if let Some(raw) = messages.get(id)? {
                let message: MessengerMessage = decode(raw.value())?;
                if message.unread {
                    out.push(message);
                }
            }
        }
        Ok(out)
    }

    /// Stores a message for later delivery and returns its id (Java: `newMessage`).
    pub fn messenger_new_message(
        &self,
        sender_id: u32,
        receiver_id: u32,
        body: &str,
        date: i64,
    ) -> Result<u32> {
        let mut w = self.write()?;
        let id = w.next_seq("messages")?;
        w.put_message(&MessengerMessage {
            id,
            receiver_id,
            sender_id,
            unread: true,
            body: body.to_string(),
            date,
        })?;
        w.commit()?;
        Ok(id)
    }

    /// Java: `markMessageRead`.
    pub fn messenger_mark_read(&self, message_id: u32) -> Result<()> {
        let mut w = self.write()?;
        w.mark_message_read(message_id, None)?;
        w.commit()
    }

    /// Java: `markMessagesRead`: only the given messages of one receiver.
    pub fn messenger_mark_all_read(&self, receiver_id: u32, message_ids: &[u32]) -> Result<()> {
        let mut w = self.write()?;
        for &id in message_ids {
            w.mark_message_read(id, Some(receiver_id))?;
        }
        w.commit()
    }

    /// Console categories of `user_id` (Java: `getCategories`).
    pub fn messenger_categories(&self, user_id: u32) -> Result<Vec<MessengerCategory>> {
        let tx = self.db.begin_read()?;
        let by_user = tx.open_table(CATEGORIES_BY_USER)?;
        let categories = tx.open_table(CATEGORIES)?;
        let mut out = Vec::new();
        for entry in by_user.range((user_id, 0)..=(user_id, u32::MAX))? {
            let id = entry?.0.value().1;
            if let Some(raw) = categories.get(id)? {
                out.push(decode(raw.value())?);
            }
        }
        Ok(out)
    }

    /// Ids of users whose name matches the SQL pattern `query%`, at most [`SEARCH_LIMIT`]
    /// (Java: `MessengerDao.search`: `username LIKE ? LIMIT 30`).
    ///
    /// Matching is case-insensitive like the MariaDB collation; `%` and `_` in the query act as
    /// wildcards, as they do in the Java query. Results come in name order, which is what
    /// MariaDB's range scan over the `username` index gives.
    pub fn messenger_search(&self, query: &str) -> Result<Vec<u32>> {
        let pattern: Vec<char> = format!("{}%", crate::name_key(query)).chars().collect();
        // Literal prefix before the first wildcard: bounds the range scan.
        let prefix: String = pattern
            .iter()
            .take_while(|c| !matches!(c, '%' | '_' | '\\'))
            .collect();
        let tx = self.db.begin_read()?;
        let table = tx.open_table(USERS_BY_NAME)?;
        let mut out = Vec::new();
        for entry in table.range(prefix.as_str()..)? {
            let (name, id) = entry?;
            let name = name.value();
            if !name.starts_with(&prefix) {
                break;
            }
            if like_matches(&pattern, &name.chars().collect::<Vec<_>>()) {
                out.push(id.value());
                if out.len() == SEARCH_LIMIT {
                    break;
                }
            }
        }
        Ok(out)
    }
}

impl Writer {
    /// Inserts or replaces a message and keeps the receiver index and the id counter in step.
    pub fn put_message(&mut self, message: &MessengerMessage) -> Result<()> {
        self.tx
            .open_table(MESSAGES)?
            .insert(message.id, encode(message)?.as_slice())?;
        self.tx
            .open_table(MESSAGES_BY_RECEIVER)?
            .insert((message.receiver_id, message.id), ())?;
        self.bump_seq("messages", message.id)
    }

    /// Inserts or replaces a console category.
    pub fn put_category(&mut self, category: &MessengerCategory) -> Result<()> {
        self.tx
            .open_table(CATEGORIES)?
            .insert(category.id, encode(category)?.as_slice())?;
        self.tx
            .open_table(CATEGORIES_BY_USER)?
            .insert((category.user_id, category.id), ())?;
        self.bump_seq("categories", category.id)
    }

    /// Imports one friendship row: `friend_id` is in the list of `owner_id`.
    pub fn put_friend(&mut self, owner_id: u32, friend_id: u32, category_id: i32) -> Result<()> {
        self.tx
            .open_table(FRIENDS)?
            .insert((owner_id, friend_id), category_id)?;
        Ok(())
    }

    /// Imports one request row.
    pub fn put_request(&mut self, from_id: u32, to_id: u32) -> Result<()> {
        self.tx.open_table(REQUESTS)?.insert((to_id, from_id), ())?;
        Ok(())
    }

    /// Sets the next AUTO_INCREMENT value of `"messages"` or `"categories"` (importer).
    pub fn set_messenger_seq(&mut self, name: &str, next: u32) -> Result<()> {
        self.tx.open_table(SEQ)?.insert(name, next)?;
        Ok(())
    }

    fn bump_seq(&mut self, name: &str, used_id: u32) -> Result<()> {
        let mut seq = self.tx.open_table(SEQ)?;
        let next = seq.get(name)?.map(|v| v.value()).unwrap_or(1);
        if used_id >= next {
            seq.insert(name, used_id + 1)?;
        }
        Ok(())
    }

    fn next_seq(&mut self, name: &str) -> Result<u32> {
        let mut seq = self.tx.open_table(SEQ)?;
        let next = seq.get(name)?.map(|v| v.value()).unwrap_or(1);
        seq.insert(name, next + 1)?;
        Ok(next)
    }

    /// Java: `MessengerDao.addCategory` (the website's friend management). Returns the new id.
    pub fn messenger_add_category(&mut self, user_id: u32, name: &str) -> Result<u32> {
        let id = self.next_seq("categories")?;
        self.put_category(&MessengerCategory {
            id,
            user_id,
            name: name.to_string(),
        })?;
        Ok(id)
    }

    /// Java: `MessengerDao.updateCategory`: renames the user's category.
    pub fn messenger_rename_category(&mut self, id: u32, user_id: u32, name: &str) -> Result<()> {
        let raw = self
            .tx
            .open_table(CATEGORIES)?
            .get(id)?
            .map(|v| v.value().to_vec());
        if let Some(raw) = raw {
            let mut c: MessengerCategory = decode(&raw)?;
            if c.user_id == user_id {
                c.name = name.to_string();
                self.put_category(&c)?;
            }
        }
        Ok(())
    }

    /// Java: `MessengerDao.deleteCategory`.
    pub fn messenger_delete_category(&mut self, id: u32, user_id: u32) -> Result<()> {
        let owned = self
            .tx
            .open_table(CATEGORIES)?
            .get(id)?
            .map(|v| decode::<MessengerCategory>(v.value()))
            .transpose()?
            .is_some_and(|c| c.user_id == user_id);
        if owned {
            self.tx.open_table(CATEGORIES)?.remove(id)?;
            self.tx
                .open_table(CATEGORIES_BY_USER)?
                .remove((user_id, id))?;
        }
        Ok(())
    }

    /// Java: `MessengerDao.updateFriendCategory`: only an existing friendship row changes.
    pub fn messenger_set_friend_category(
        &mut self,
        owner_id: u32,
        friend_id: u32,
        category_id: i32,
    ) -> Result<()> {
        let mut friends = self.tx.open_table(FRIENDS)?;
        if friends.get((owner_id, friend_id))?.is_some() {
            friends.insert((owner_id, friend_id), category_id)?;
        }
        Ok(())
    }

    /// Java: `MessengerDao.resetFriendCategories`: the owner's friends in `category_id` go back to 0.
    pub fn messenger_reset_friend_categories(
        &mut self,
        owner_id: u32,
        category_id: i32,
    ) -> Result<()> {
        let mut friends = self.tx.open_table(FRIENDS)?;
        let mut changed = Vec::new();
        for entry in friends.range((owner_id, 0)..=(owner_id, u32::MAX))? {
            let (k, v) = entry?;
            if v.value() == category_id {
                changed.push(k.value());
            }
        }
        for k in changed {
            friends.insert(k, 0)?;
        }
        Ok(())
    }

    /// Marks a message read, only if it belongs to `receiver` when one is given.
    fn mark_message_read(&mut self, id: u32, receiver: Option<u32>) -> Result<()> {
        let raw = self
            .tx
            .open_table(MESSAGES)?
            .get(id)?
            .map(|v| v.value().to_vec());
        if let Some(raw) = raw {
            let mut message: MessengerMessage = decode(&raw)?;
            if receiver.is_none_or(|r| r == message.receiver_id) {
                message.unread = false;
                self.put_message(&message)?;
            }
        }
        Ok(())
    }
}

/// SQL `LIKE` with `%`, `_` and `\` as the escape character, over lowercase text.
///
/// Greedy matching that backtracks only to the last `%`: O(pattern × text) at worst. (A naive
/// recursive matcher is exponential in the number of `%`, so one search for `%%%…%x` could pin a
/// worker thread for minutes.)
fn like_matches(pattern: &[char], text: &[char]) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Token {
        Any,
        One,
        Lit(char),
    }
    let mut tokens = Vec::with_capacity(pattern.len());
    let mut i = 0;
    while i < pattern.len() {
        tokens.push(match pattern[i] {
            // Consecutive `%` are one `%`.
            '%' if tokens.last() == Some(&Token::Any) => {
                i += 1;
                continue;
            }
            '%' => Token::Any,
            '_' => Token::One,
            '\\' if i + 1 < pattern.len() => {
                i += 1;
                Token::Lit(pattern[i])
            }
            c => Token::Lit(c),
        });
        i += 1;
    }

    let (mut p, mut t) = (0, 0);
    // Position after the last `%` seen, and the text position it is currently matched up to.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match tokens.get(p) {
            Some(Token::Any) => {
                star = Some((p + 1, t));
                p += 1;
            }
            Some(Token::One) => {
                p += 1;
                t += 1;
            }
            Some(Token::Lit(c)) if *c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                // Let the last `%` swallow one more character and retry from there.
                Some((sp, st)) => {
                    star = Some((sp, st + 1));
                    p = sp;
                    t = st + 1;
                }
                None => return false,
            },
        }
    }
    tokens[p..].iter().all(|tok| *tok == Token::Any)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::User;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, store)
    }

    fn user(id: u32, name: &str) -> User {
        User {
            id,
            username: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn friends_and_requests() {
        let (_dir, store) = temp_store();
        store.messenger_new_request(1, 2).unwrap();
        store.messenger_new_request(3, 2).unwrap();
        store.messenger_new_request(2, 2).unwrap(); // to oneself: ignored
        assert_eq!(store.messenger_requests(2).unwrap(), vec![1, 3]);
        assert!(store.messenger_request_exists(1, 2).unwrap());
        assert!(!store.messenger_request_exists(2, 1).unwrap());

        store.messenger_remove_request(1, 2).unwrap();
        assert_eq!(store.messenger_requests(2).unwrap(), vec![3]);
        store.messenger_remove_all_requests(2).unwrap();
        assert!(store.messenger_requests(2).unwrap().is_empty());

        store.messenger_add_friend(1, 2).unwrap();
        store.messenger_add_friend(2, 1).unwrap();
        assert_eq!(store.messenger_friends(1).unwrap(), vec![(2, 0)]);
        assert!(store.messenger_friend_exists(2, 1).unwrap());
        store.messenger_remove_friend(1, 2).unwrap();
        assert!(store.messenger_friends(1).unwrap().is_empty());
        assert_eq!(store.messenger_friends(2).unwrap(), vec![(1, 0)]);
    }

    #[test]
    fn messages_get_increasing_ids_and_can_be_read() {
        let (_dir, store) = temp_store();
        let a = store.messenger_new_message(1, 2, "hi", 100).unwrap();
        let b = store.messenger_new_message(1, 2, "there", 101).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(store.messenger_unread(2).unwrap().len(), 2);
        store.messenger_mark_read(a).unwrap();
        let unread = store.messenger_unread(2).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].body, "there");
        store.messenger_mark_all_read(9, &[b]).unwrap(); // wrong receiver: untouched
        assert_eq!(store.messenger_unread(2).unwrap().len(), 1);
        store.messenger_mark_all_read(2, &[b]).unwrap();
        assert!(store.messenger_unread(2).unwrap().is_empty());
    }

    #[test]
    fn search_is_a_prefix_like() {
        let (_dir, store) = temp_store();
        for (id, name) in [(1, "Alice"), (2, "alan"), (3, "bob"), (4, "a_b")] {
            store.put_user(&user(id, name)).unwrap();
        }
        assert_eq!(store.messenger_search("al").unwrap(), vec![2, 1]);
        assert_eq!(store.messenger_search("AL").unwrap(), vec![2, 1]);
        assert_eq!(store.messenger_search("b").unwrap(), vec![3]);
        // An unescaped underscore is a wildcard, an escaped one is literal.
        assert_eq!(store.messenger_search("a_").unwrap(), vec![4, 2, 1]);
        assert_eq!(store.messenger_search("a\\_").unwrap(), vec![4]);
        assert_eq!(store.messenger_search("").unwrap(), vec![4, 2, 1, 3]);
        assert!(store.messenger_search("zz").unwrap().is_empty());
        assert_eq!(store.messenger_search("%b").unwrap(), vec![4, 3]);
        assert_eq!(store.messenger_search("%i%e").unwrap(), vec![1]);
    }

    /// The obvious recursive definition of `LIKE`, as a reference for [`like_matches`].
    fn like_reference(pattern: &[char], text: &[char]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((&'%', rest)) => (0..=text.len()).any(|skip| like_reference(rest, &text[skip..])),
            Some((&'_', rest)) => !text.is_empty() && like_reference(rest, &text[1..]),
            Some((&'\\', rest)) if !rest.is_empty() => {
                text.first() == Some(&rest[0]) && like_reference(&rest[1..], &text[1..])
            }
            Some((c, rest)) => text.first() == Some(c) && like_reference(rest, &text[1..]),
        }
    }

    #[test]
    fn like_matches_agrees_with_the_reference() {
        // Every pattern of up to 5 symbols over this alphabet, against every text of up to 4.
        let symbols = ['%', '_', '\\', 'a', 'b'];
        let letters = ['a', 'b', '_', '%', '\\'];
        fn all(alphabet: &[char], max: usize) -> Vec<Vec<char>> {
            let mut out = vec![vec![]];
            let mut layer = vec![vec![]];
            for _ in 0..max {
                layer = layer
                    .iter()
                    .flat_map(|w: &Vec<char>| {
                        alphabet.iter().map(move |&c| {
                            let mut w = w.clone();
                            w.push(c);
                            w
                        })
                    })
                    .collect();
                out.extend(layer.iter().cloned());
            }
            out
        }
        let texts = all(&letters, 4);
        for pattern in all(&symbols, 5) {
            for text in &texts {
                assert_eq!(
                    like_matches(&pattern, text),
                    like_reference(&pattern, text),
                    "pattern {pattern:?} text {text:?}"
                );
            }
        }
    }

    #[test]
    fn many_wildcards_stay_fast() {
        let pattern: Vec<char> = format!("{}Q%", "%".repeat(200)).chars().collect();
        let text: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123".chars().collect();
        let start = std::time::Instant::now();
        assert!(!like_matches(&pattern, &text));
        let pattern: Vec<char> = "_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%_%Z"
            .chars()
            .collect();
        assert!(!like_matches(&pattern, &text));
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
    }
}
