//! What Havana-Web shows from memory instead of the database: the `Watchdog`'s lists, refreshed
//! every 30 seconds, and the `WebCache` regions underneath with their time to live. Pages built
//! from them are as stale as Java's.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use rusbbo_storage::{Article, ForumReply, ForumThread, Group, Room, RoomEvent, User};

use super::data;
use crate::world::World;

/// A `CacheRegion`: values expire `ttl` after they were computed.
pub struct TtlCache<K, V> {
    ttl: Duration,
    map: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        let now = Instant::now();
        if let Some((t, v)) = self.map.lock().get(&key)
            && now.duration_since(*t) < self.ttl
        {
            return v.clone();
        }
        let v = compute();
        self.map.lock().insert(key, (now, v.clone()));
        v
    }

    pub fn invalidate_all(&self) {
        self.map.lock().clear();
    }
}

/// A recent discussion (`CommunityDao.getRecentDiscussions`): the thread, its reply count and
/// when its last reply was posted.
#[derive(Clone)]
pub struct Discussion {
    pub thread: ForumThread,
    pub replies: i64,
    pub last_message_at: i64,
}

/// A high score row: (player name, score, position).
pub type ScoreRow = (String, i64, i64);

/// Java: `WebCache`'s regions.
pub struct Caches {
    pub tag_cloud: TtlCache<usize, Vec<(String, i64)>>,
    pub news: TtlCache<(usize, bool), Vec<Article>>,
    /// Every article, for the news pages (same region as `news`).
    pub news_all: TtlCache<(), Vec<Article>>,
    pub recommended_groups: TtlCache<bool, Vec<Group>>,
    pub recommended_rooms: TtlCache<bool, Vec<Room>>,
    pub popular_rooms: TtlCache<(usize, usize), Vec<Room>>,
    pub highscores: TtlCache<(i32, i64, bool), Vec<ScoreRow>>,
    pub active_habbos: TtlCache<(), Vec<User>>,
    pub last_visits: TtlCache<(), i64>,
}

impl Default for Caches {
    fn default() -> Self {
        let s = Duration::from_secs;
        Self {
            tag_cloud: TtlCache::new(s(300)),
            news: TtlCache::new(s(120)),
            news_all: TtlCache::new(s(120)),
            recommended_groups: TtlCache::new(s(60)),
            recommended_rooms: TtlCache::new(s(60)),
            popular_rooms: TtlCache::new(s(60)),
            highscores: TtlCache::new(s(60)),
            active_habbos: TtlCache::new(s(60)),
            last_visits: TtlCache::new(s(60)),
        }
    }
}

/// Java: `Watchdog`'s static lists.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub staff_pick_groups: Vec<Group>,
    pub recommended_groups: Vec<Group>,
    pub events: Vec<RoomEvent>,
    pub staff_pick_rooms: Vec<Room>,
    pub recommended_rooms: Vec<Room>,
    pub hidden_recommended_rooms: Vec<Room>,
    pub recent_discussions: Vec<Discussion>,
    pub next_recent_discussions: Vec<Discussion>,
    pub tag_cloud_10: Vec<(String, i64)>,
    pub tag_cloud_20: Vec<(String, i64)>,
    pub news: Vec<Article>,
    pub news_staff: Vec<Article>,
    pub users_online: i64,
    pub visits: i64,
}

/// The watchdog's state.
#[derive(Default)]
pub struct Watchdog {
    pub caches: Caches,
    pub snapshot: RwLock<Arc<Snapshot>>,
}

/// Java: `CommunityDao.getRecentDiscussions`: threads by their latest reply, newest first.
pub fn recent_discussions(world: &World, limit: usize, offset: usize) -> Vec<Discussion> {
    let threads = world.store.records::<ForumThread>().unwrap_or_default();
    let replies = world.store.records::<ForumReply>().unwrap_or_default();
    let mut rows: Vec<Discussion> = threads
        .into_iter()
        .filter_map(|t| {
            let mine: Vec<&ForumReply> = replies.iter().filter(|r| r.thread_id == t.id).collect();
            let last = mine.iter().max_by_key(|r| r.id)?;
            Some(Discussion {
                last_message_at: last.created_at,
                replies: mine.len() as i64,
                thread: t,
            })
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.last_message_at));
    rows.into_iter().skip(offset).take(limit).collect()
}

impl Watchdog {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().clone()
    }

    pub fn tag_cloud(&self, world: &World, size: usize) -> Vec<(String, i64)> {
        self.caches
            .tag_cloud
            .get(size, || data::popular_tags(world, size))
    }

    pub fn news_top(&self, world: &World, limit: usize, unpublished: bool) -> Vec<Article> {
        self.caches.news.get((limit, unpublished), || {
            data::top_news(world, limit, unpublished)
        })
    }

    pub fn recommended_groups(&self, world: &World, staff: bool) -> Vec<Group> {
        self.caches
            .recommended_groups
            .get(staff, || data::recommended_groups(world, staff))
    }

    pub fn recommended_rooms(&self, world: &World, staff: bool) -> Vec<Room> {
        self.caches
            .recommended_rooms
            .get(staff, || data::recommended_rooms(world, staff))
    }

    pub fn popular_rooms(&self, world: &World, limit: usize, offset: usize) -> Vec<Room> {
        self.caches.popular_rooms.get((limit, offset), || {
            data::popular_rooms(world, limit, offset)
        })
    }

    /// `WebCache.randomHabbos`: the pool is cached, the pick is new each time.
    pub fn random_habbos(&self, world: &World, count: usize) -> Vec<User> {
        use rand::seq::SliceRandom;
        let mut pool = self
            .caches
            .active_habbos
            .get((), || data::recently_active(world));
        pool.shuffle(&mut rand::rng());
        pool.truncate(count);
        pool
    }

    pub fn last_visits(&self, world: &World) -> i64 {
        self.caches
            .last_visits
            .get((), || world.store.web_last_visits().unwrap_or(0))
    }

    /// One watchdog tick's list refresh (every 30 seconds).
    pub fn refresh(&self, world: &World) {
        let now = super::beans::now();
        let snapshot = Snapshot {
            users_online: world.players_online() as i64,
            visits: self.last_visits(world),
            events: world
                .store
                .room_events()
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.expire_time > now)
                .collect(),
            recommended_groups: self.recommended_groups(world, false),
            staff_pick_groups: self.recommended_groups(world, true),
            staff_pick_rooms: self.recommended_rooms(world, true),
            recommended_rooms: self.popular_rooms(world, 5, 0),
            hidden_recommended_rooms: self.popular_rooms(world, 5, 5),
            recent_discussions: recent_discussions(world, 10, 0),
            next_recent_discussions: recent_discussions(world, 10, 10),
            tag_cloud_10: self.tag_cloud(world, 10),
            tag_cloud_20: self.tag_cloud(world, 20),
            news: self.news_top(world, 5, false),
            news_staff: self.news_top(world, 5, true),
        };
        *self.snapshot.write() = Arc::new(snapshot);
    }

    /// Java: `NewsDao.publishFutureArticles` and `WebCache.invalidateNews` (every 5 minutes).
    pub fn publish_future_articles(&self, world: &World) {
        let now = super::beans::now();
        for mut a in world
            .store
            .records_where::<Article>(|a| {
                !a.is_published && a.is_future_published && now > a.created_at
            })
            .unwrap_or_default()
        {
            a.is_published = true;
            let _ = world.store.put_record(&a);
        }
        self.invalidate_news();
    }

    /// Java: `WebCache.invalidateNews`.
    pub fn invalidate_news(&self) {
        self.caches.news.invalidate_all();
        self.caches.news_all.invalidate_all();
    }

    pub fn all_news(&self, world: &World) -> Vec<Article> {
        self.caches
            .news_all
            .get((), || world.store.records::<Article>().unwrap_or_default())
    }
}

/// The watchdog task (Java: scheduled every second, from one second after start).
pub async fn run(web: Arc<super::Web>) {
    let mut counter: u64 = 0;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let w = web.clone();
        let c = counter;
        let _ = tokio::task::spawn_blocking(move || {
            if c % 300 == 0 {
                w.watchdog.publish_future_articles(&w.world);
            }
            if c % 3600 == 0 {
                super::email::clean_recovery_codes(&w.world);
            }
            if c % 30 == 0 {
                w.watchdog.refresh(&w.world);
            }
        })
        .await;
        counter += 1;
    }
}
