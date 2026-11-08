//! The website's tables (Havana-Web's `cms_*`, `site_*`, `homes_*`, `groups_*` and friends).
//!
//! Most of them are small and read whole by the pages that show them, so they share one generic
//! shape: a [`Record`] type keyed by a `u32` id, stored with postcard. Queries filter a scan of
//! the table, which is what the Java DAOs' `SELECT ... WHERE` amounts to at these sizes.

use redb::{ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Result, Store, USERS, User, Writer, decode, encode};

/// `AUTO_INCREMENT` counters by table name (the next id MariaDB would hand out).
const WEB_META: TableDefinition<&str, u32> = TableDefinition::new("web_meta");

/// A row of one of the website's tables.
pub trait Record: Serialize + DeserializeOwned + Clone {
    const TABLE: TableDefinition<'static, u32, &'static [u8]>;
    fn id(&self) -> u32;
    fn set_id(&mut self, id: u32);
}

/// Each `Record` table, for `create_tables`.
macro_rules! record_tables {
    ($($t:ty),* $(,)?) => {
        pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
            tx.open_table(WEB_META)?;
            $( tx.open_table(<$t as Record>::TABLE)?; )*
            Ok(())
        }
    };
}

/// Declares a [`Record`] struct (with an `id: u32` field) and its table.
macro_rules! record {
    ($(#[$meta:meta])* $name:ident, $table:literal { $($(#[$fmeta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            pub id: u32,
            $($(#[$fmeta])* pub $field: $ty),*
        }
        impl Record for $name {
            const TABLE: TableDefinition<'static, u32, &'static [u8]> = TableDefinition::new($table);
            fn id(&self) -> u32 { self.id }
            fn set_id(&mut self, id: u32) { self.id = id; }
        }
    };
}

record!(
    /// `users_tags` (Java: `TagDao`): a tag on a user, a room or a group. The table has no key in
    /// MariaDB; ids here are the import order.
    Tag, "users_tags" {
        user_id: u32,
        room_id: u32,
        group_id: u32,
        tag: String,
        created_at: i64,
    }
);

record!(
    /// `site_articles`, with its `site_articles_categories` (Java: `NewsDao`).
    Article, "site_articles" {
        title: String,
        author_id: u32,
        author_override: String,
        short_story: String,
        full_story: String,
        topstory: String,
        topstory_override: String,
        article_image: String,
        is_published: bool,
        is_future_published: bool,
        views: i32,
        created_at: i64,
        /// Category ids, in `GROUP_CONCAT` order.
        categories: Vec<u32>,
    }
);

record!(
    /// `article_categories`.
    ArticleCategory, "article_categories" {
        label: String,
        category_index: String,
    }
);

record!(
    /// `cms_alerts` (Java: `AlertsDao`): the notices on `/me`.
    Alert, "cms_alerts" {
        user_id: u32,
        /// `HC_EXPIRED`, `PRESENT`, `TUTOR_SCORE` or `CREDIT_DONATION`.
        alert_type: String,
        message: String,
        is_disabled: bool,
        created_at: i64,
    }
);

record!(
    /// `cms_minimail` (Java: `MinimailDao`).
    Minimail, "cms_minimail" {
        target_id: u32,
        sender_id: u32,
        to_id: u32,
        is_read: bool,
        subject: String,
        message: String,
        date_sent: i64,
        conversation_id: u32,
        is_trash: bool,
        is_deleted: bool,
    }
);

record!(
    /// `cms_recommended` (Java: `RecommendedDao`): staff picks and recommended groups and rooms.
    Recommended, "cms_recommended" {
        recommended_id: u32,
        /// `GROUP` or `ROOM`.
        kind: String,
        is_staff_pick: bool,
    }
);

record!(
    /// `groups_details` (Java: `GroupDao`).
    Group, "groups_details" {
        name: String,
        description: String,
        owner_id: u32,
        room_id: u32,
        badge: String,
        recommended: i32,
        background: String,
        views: i32,
        topics: i32,
        group_type: i32,
        forum_type: i32,
        forum_permission: i32,
        alias: Option<String>,
        created_at: i64,
    }
);

record!(
    /// `groups_memberships` (Java: `GroupMemberDao`); ids are synthetic.
    GroupMember, "groups_memberships" {
        user_id: u32,
        group_id: u32,
        /// `member_rank`: 1 member, 2 administrator, 3 owner.
        member_rank: i32,
        is_pending: bool,
        created_at: i64,
    }
);

record!(
    /// `groups_edit_sessions`; ids are synthetic.
    GroupEditSession, "groups_edit_sessions" {
        user_id: u32,
        group_id: u32,
        expire: i64,
    }
);

record!(
    /// `cms_forum_threads` (Java: `GroupDiscussionDao`).
    ForumThread, "cms_forum_threads" {
        topic_title: String,
        poster_id: u32,
        is_open: bool,
        is_stickied: bool,
        views: i32,
        group_id: u32,
        created_at: i64,
        modified_at: i64,
    }
);

record!(
    /// `cms_forum_replies` (Java: `ReplyDao`).
    ForumReply, "cms_forum_replies" {
        thread_id: u32,
        message: String,
        poster_id: u32,
        is_edited: bool,
        is_deleted: bool,
        created_at: i64,
        modified_at: i64,
    }
);

record!(
    /// `cms_forums_read_replies`; ids are synthetic.
    ForumRead, "cms_forums_read_replies" {
        user_id: u32,
        reply_id: u32,
    }
);

record!(
    /// `cms_guestbook_entries` (Java: `GuestbookDao`).
    GuestbookEntry, "cms_guestbook_entries" {
        user_id: u32,
        home_id: u32,
        group_id: u32,
        message: String,
        created_at: i64,
    }
);

record!(
    /// `cms_stickers` (Java: `WidgetDao`): stickers, notes, widgets and backgrounds owned or
    /// placed on a home or a group page.
    Sticker, "cms_stickers" {
        user_id: u32,
        x: String,
        y: String,
        z: String,
        sticker_id: u32,
        skin_id: i32,
        group_id: i32,
        text: String,
        is_placed: bool,
        extra_data: String,
    }
);

record!(
    /// `cms_stickers_catalogue` (Java: `StoreDao`).
    StickerProduct, "cms_stickers_catalogue" {
        name: String,
        description: String,
        /// `type`.
        kind: String,
        data: String,
        price: i32,
        amount: i32,
        category_id: u32,
        min_rank: i32,
        widget_type: i32,
        /// The row's place in `SELECT * FROM cms_stickers_catalogue` (a MyISAM table read in
        /// storage order, which is not id order): the order Java lists products in.
        position: u32,
    }
);

record!(
    /// `cms_stickers_categories`.
    StickerCategory, "cms_stickers_categories" {
        name: String,
        min_rank: i32,
        category_type: i32,
    }
);

record!(
    /// `homes_details`, keyed by the user id.
    Home, "homes_details" {
        background: String,
    }
);

record!(
    /// `homes_edit_sessions`, keyed by the user id.
    HomeEditSession, "homes_edit_sessions" {
        expire: i64,
    }
);

record!(
    /// `homes_ratings`; ids are synthetic.
    HomeRating, "homes_ratings" {
        user_id: u32,
        home_id: u32,
        rating: i32,
    }
);

record!(
    /// `users_wardrobes`; ids are synthetic.
    Wardrobe, "users_wardrobes" {
        user_id: u32,
        slot_id: i32,
        sex: String,
        figure: String,
    }
);

record!(
    /// `vouchers` with its `vouchers_items`; ids are synthetic.
    Voucher, "vouchers" {
        code: String,
        credits: i32,
        expiry_date: Option<i64>,
        is_single_use: bool,
        allow_new_users: bool,
        /// `vouchers_items.catalogue_sale_code`.
        items: Vec<String>,
    }
);

record!(
    /// `vouchers_history`; ids are synthetic.
    VoucherUse, "vouchers_history" {
        code: String,
        user_id: u32,
        used_at: i64,
        credits_redeemed: Option<i32>,
        items_redeemed: Option<String>,
    }
);

record!(
    /// `users_referred`; ids are synthetic.
    Referral, "users_referred" {
        user_id: u32,
        referred_id: u32,
    }
);

record!(
    /// `users_club_gifts`; ids are synthetic.
    ClubGift, "users_club_gifts" {
        user_id: u32,
        sprite: String,
        date_received: i64,
    }
);

record!(
    /// `room_visits`; ids are synthetic.
    RoomVisit, "room_visits" {
        room_id: u32,
        user_id: u32,
        visited_at: i64,
    }
);

record!(
    /// `catalogue_collectables`, keyed by `store_page`.
    Collectable, "catalogue_collectables" {
        admin_page: i32,
        expiry: i64,
        lifetime: i64,
        current_position: i32,
        class_names: String,
    }
);

record!(
    /// `rooms_ads` (Java: `AdManager`, `AdvertisementsDao`).
    RoomAd, "rooms_ads" {
        is_loading_ad: bool,
        room_id: u32,
        url: Option<String>,
        image: String,
        enabled: bool,
    }
);

record!(
    /// `users_statistics.verify_code` (Java: `VerifyDao`), keyed by the user id.
    VerifyCode, "users_verify_codes" {
        code: String,
    }
);

record_tables!(
    RoomAd,
    VerifyCode,
    Tag,
    Article,
    ArticleCategory,
    Alert,
    Minimail,
    Recommended,
    Group,
    GroupMember,
    GroupEditSession,
    ForumThread,
    ForumReply,
    ForumRead,
    GuestbookEntry,
    Sticker,
    StickerProduct,
    StickerCategory,
    Home,
    HomeEditSession,
    HomeRating,
    Wardrobe,
    Voucher,
    VoucherUse,
    Referral,
    ClubGift,
    RoomVisit,
    Collectable,
);

impl Store {
    pub fn record<R: Record>(&self, id: u32) -> Result<Option<R>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(R::TABLE)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Every row, by id.
    pub fn records<R: Record>(&self) -> Result<Vec<R>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(R::TABLE)?;
        let mut out = Vec::with_capacity(table.len()? as usize);
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// The rows `keep` accepts, by id.
    pub fn records_where<R: Record>(&self, keep: impl Fn(&R) -> bool) -> Result<Vec<R>> {
        Ok(self
            .records::<R>()?
            .into_iter()
            .filter(|r| keep(r))
            .collect())
    }

    pub fn put_record<R: Record>(&self, r: &R) -> Result<()> {
        let mut w = self.write()?;
        w.put_record(r)?;
        w.commit()
    }

    /// Inserts `r` with the next id (`AUTO_INCREMENT`); returns the id.
    pub fn insert_record<R: Record>(&self, r: &mut R) -> Result<u32> {
        let mut w = self.write()?;
        let id = w.insert_record(r)?;
        w.commit()?;
        Ok(id)
    }

    pub fn delete_record<R: Record>(&self, id: u32) -> Result<bool> {
        let mut w = self.write()?;
        let found = w.delete_record::<R>(id)?;
        w.commit()?;
        Ok(found)
    }

    /// Every user (for the website's searches and counts).
    pub fn users(&self) -> Result<Vec<User>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(USERS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `SiteDao.getLastVisits`: users online in the last 30 days.
    pub fn web_last_visits(&self) -> Result<i64> {
        let since = now() - 30 * 24 * 3600;
        Ok(self
            .users()?
            .iter()
            .filter(|u| u.last_online > since)
            .count() as i64)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Writer {
    pub fn record<R: Record>(&mut self, id: u32) -> Result<Option<R>> {
        let table = self.tx.open_table(R::TABLE)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    pub fn put_record<R: Record>(&mut self, r: &R) -> Result<()> {
        let bytes = encode(r)?;
        self.tx
            .open_table(R::TABLE)?
            .insert(r.id(), bytes.as_slice())?;
        Ok(())
    }

    pub fn insert_record<R: Record>(&mut self, r: &mut R) -> Result<u32> {
        let last = {
            let table = self.tx.open_table(R::TABLE)?;
            table.last()?.map(|(k, _)| k.value()).unwrap_or(0)
        };
        let id = self.take_next_id(R::TABLE.name(), last)?;
        r.set_id(id);
        self.put_record(r)?;
        Ok(id)
    }

    pub fn delete_record<R: Record>(&mut self, id: u32) -> Result<bool> {
        Ok(self.tx.open_table(R::TABLE)?.remove(id)?.is_some())
    }

    /// The next `AUTO_INCREMENT` id of `table`, whose highest id now is `last`.
    fn take_next_id(&mut self, table: &str, last: u32) -> Result<u32> {
        let mut meta = self.tx.open_table(WEB_META)?;
        let counter = meta.get(table)?.map(|v| v.value()).unwrap_or(0);
        let id = counter.max(last + 1);
        meta.insert(table, id + 1)?;
        Ok(id)
    }

    /// Sets a table's `AUTO_INCREMENT` counter (the importer copies MariaDB's).
    pub fn set_auto_increment(&mut self, table: &str, next: u32) -> Result<()> {
        self.tx.open_table(WEB_META)?.insert(table, next)?;
        Ok(())
    }

    /// The id the next `INSERT INTO users` gets (Java: `RegisterDao.newUser`).
    pub fn next_user_id(&mut self) -> Result<u32> {
        let last = self
            .tx
            .open_table(USERS)?
            .last()?
            .map(|(k, _)| k.value())
            .unwrap_or(0);
        self.take_next_id("users", last)
    }
}
