//! The Multi User Server (Java: `server/mus`, `MusConnectionHandler`): the Shockwave camera's
//! connection, on the game port + 1 (`--mus`). A client logs on, names its user (who must be
//! online from the same IP), and then saves photos (`PHOTOTXT`, `BINDATA`) and loads the photos
//! hanging in its room (`GETBINDATA`). Photos are stored in `items_photos` (`rusbbo_storage::Photo`).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use rusbbo_protocol::mus::{self, Content, Decoded, MusMessage, Prop, types};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

use crate::game::{long_date, now};
use crate::messages::outgoing;
use crate::world::World;

/// Java: `MusClient`: the user the connection logged in as (0 until `LOGIN`), and the caption of
/// the next photo (`PHOTOTXT`; Java's `null` until set).
#[derive(Default)]
struct Client {
    user_id: u32,
    photo_text: Option<String>,
}

pub async fn serve(world: Arc<World>, bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    info!("Multi User Server (MUS) is listening on {bind}");
    loop {
        let (stream, _) = listener.accept().await?;
        let world = world.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(world, stream).await {
                tracing::debug!("MUS connection: {e:#}");
            }
        });
    }
}

/// What a message makes the connection do.
enum Reply {
    Send(Vec<MusMessage>),
    Close,
}

async fn connection(world: Arc<World>, mut stream: TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let ip = stream.peer_addr()?.ip().to_string();
    let mut client = Client::default();
    let mut decoder = mus::Decoder::default();
    let mut buf = vec![0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        decoder.push(&buf[..n]);
        while let Some(decoded) = decoder.next_message() {
            let Decoded::Message(msg) = decoded else {
                return Ok(()); // Java closes the channel.
            };
            match handle(&world, &mut client, &ip, &msg) {
                Reply::Send(replies) => {
                    for mut reply in replies {
                        // Java: `MusNetworkEncoder` sets these on every message.
                        reply.sender = "System".into();
                        reply.receivers = vec!["*".into()];
                        reply.timestamp = now() as i32;
                        stream.write_all(&mus::encode(&reply)).await?;
                    }
                }
                Reply::Close => return Ok(()),
            }
        }
    }
}

fn handle(world: &World, client: &mut Client, ip: &str, msg: &MusMessage) -> Reply {
    let mut replies = Vec::new();
    match msg.subject.as_str() {
        "Logon" => {
            replies.push(MusMessage::new(
                "Logon",
                Content::Str("Havana: Habbo Hotel shockwave emulator".into()),
            ));
            replies.push(MusMessage::new("HELLO", Content::Str(String::new())));
        }
        "LOGIN" => {
            // Java: `getContentString().split(" ", 2)` (a message that is not a string throws).
            let Some(content) = msg.string() else {
                return Reply::Send(replies);
            };
            let first = content.split(' ').next().unwrap_or("");
            if first.is_empty() || !first.chars().all(|c| c.is_ascii_digit()) {
                return Reply::Send(replies);
            }
            let Ok(user_id) = first.parse::<i32>() else {
                return Reply::Send(replies); // Integer.valueOf throws.
            };
            let same_ip = u32::try_from(user_id)
                .ok()
                .and_then(|id| world.player_by_id(id))
                .is_some_and(|s| s.player.lock().ip_address == ip);
            if !same_ip {
                return Reply::Close;
            }
            client.user_id = user_id as u32;
        }
        "PHOTOTXT" => {
            if client.user_id > 0 {
                let Some(content) = msg.string() else {
                    return Reply::Send(replies);
                };
                // Java: `substring(1)` (throws on an empty caption), filtered, at most 105.
                let mut chars = content.chars();
                if chars.next().is_none() {
                    return Reply::Send(replies);
                }
                let bytes: Vec<u8> = chars.map(|c| c as u32 as u8).collect();
                let mut text: String = crate::game::room::filter_input(&bytes)
                    .iter()
                    .map(|&b| char::from(b))
                    .collect();
                if text.chars().count() >= 105 {
                    text = text.chars().take(105).collect();
                }
                client.photo_text = Some(text);
            }
        }
        "BINDATA" => {
            if let Some(saved) = save_photo(world, client, msg) {
                replies.push(saved);
            }
        }
        "GETBINDATA" => {
            if let Some(photo) = load_photo(world, client, msg) {
                replies.push(photo);
            }
        }
        _ => {}
    }
    Reply::Send(replies)
}

/// Java: `BINDATA`: a photo item for the user's hand with the date and caption, its image in
/// `items_photos`, one film less. `BINDATA_SAVED` is the MUS reply; the hand and `FILM` go to
/// the game connection.
fn save_photo(world: &World, client: &Client, msg: &MusMessage) -> Option<MusMessage> {
    let session = world.player_by_id(client.user_id)?;
    if client.user_id < 1 || session.player.lock().room_id.is_none() {
        return None;
    }
    let Content::Props(props) = &msg.content else {
        return None; // Java's getContentPropList() is null: it throws.
    };
    let t = now();
    let checksum = mus::prop_int(props, "cs");
    let image = mus::prop_bytes(props, "image");
    let caption = client.photo_text.as_deref().unwrap_or("null");
    // Java: `getDefinitionBySprite("photo").getId()` throws without one.
    let definition = world.items.definition_by_sprite("photo")?;
    let written = (|| -> anyhow::Result<rusbbo_storage::Item> {
        let mut w = world.store.write()?;
        let row = rusbbo_storage::Item::with_defaults(
            client.user_id,
            definition.record.id,
            format!("{}\r{caption}", long_date(t)),
            t,
        );
        let item = w.insert_item(&row, t)?;
        w.put_photo(&rusbbo_storage::Photo {
            photo_id: item.id,
            user_id: i64::from(client.user_id),
            timestamp: t,
            data: image,
            checksum,
            is_active: true,
        })?;
        w.commit()?;
        Ok(item)
    })();
    let item = match written {
        Ok(item) => item,
        Err(e) => {
            error!(user = client.user_id, "saving a photo: {e}");
            return None;
        }
    };
    let reply = MusMessage::new("BINDATA_SAVED", Content::Str(client.user_id.to_string()));
    let mut p = session.player.lock();
    if let Some(item) = world.items.make_item(item) {
        p.inventory.add_item(item);
    }
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(outgoing::inventory::inventory(&hand));
    // Java: `CurrencyDao.decreaseFilm(details, 1)`: nothing taken without film, never below 0.
    if let Some(details) = p.details.as_mut() {
        let film = (details.film - i32::from(details.film > 0)).max(0);
        details.film = film;
        if let Err(e) = world.store.update_user(details.id, |u| u.film = film) {
            error!(user = details.id, "saving film: {e}");
        }
        session.send(outgoing::user::film(film));
    }
    Some(reply)
}

/// Java: `GETBINDATA`: a photo hanging in the user's room, by its id in the room.
fn load_photo(world: &World, client: &Client, msg: &MusMessage) -> Option<MusMessage> {
    // Java: `Integer.parseInt(getContentString().split(" ")[0])`, which throws otherwise.
    let photo_id: i32 = msg.string()?.split(' ').next()?.parse().ok()?;
    let session = world.player_by_id(client.user_id)?;
    let room = crate::game::room::current_room(world, &session)?;
    let database_id = {
        let r = room.lock();
        let idx = r.mapping.item_index(photo_id)?;
        r.mapping.items[idx]
            .flat
            .as_ref()
            .map_or(0, |f| f.record.id)
    };
    let photo = world.store.active_photo(database_id).ok().flatten()?;
    if client.user_id < 1 {
        return None;
    }
    Some(MusMessage::new(
        "BINARYDATA",
        Content::Props(vec![
            Prop::bytes("image", types::MEDIA, photo.data),
            Prop::string("time", &long_date(photo.timestamp)),
            Prop::int("cs", photo.checksum),
        ]),
    ))
}
