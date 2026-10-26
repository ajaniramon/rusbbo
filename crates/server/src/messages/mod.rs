//! Message handling (Java: `org.alexdev.havana.messages`). `incoming` mirrors
//! `messages/incoming/<package>`, `outgoing` mirrors `messages/outgoing/<package>`.

pub mod incoming;
pub mod outgoing;

use std::sync::Arc;

use havana_protocol::{Message, ids};
use tracing::{debug, warn};

use crate::session::Session;
use crate::world::World;

/// Handler signature (Java: `MessageEvent.handle`). Synchronous on purpose: see docs/DESIGN.md.
pub type Handler =
    fn(&World, &Arc<Session>, &mut havana_protocol::body::ClientReader) -> Result<(), Malformed>;

/// A body that did not parse (Java: `MalformedPacketException`); the message is dropped.
#[derive(Debug)]
pub struct Malformed;

/// Dispatches one client message (Java: `MessageHandler.handleRequest`).
pub fn handle(world: &World, session: &Arc<Session>, msg: Message) {
    let name = ids::client_name(msg.header).unwrap_or("Unknown");
    debug!(conn = session.id, header = msg.header, name, "received");
    let Some(handler) = incoming::handler(msg.header) else {
        return;
    };
    let mut reader = havana_protocol::body::ClientReader::new(&msg.body);
    if handler(world, session, &mut reader).is_err() {
        warn!(
            conn = session.id,
            header = msg.header,
            name,
            "malformed message"
        );
    }
}
