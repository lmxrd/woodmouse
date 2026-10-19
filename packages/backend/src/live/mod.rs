//! Live collaborative editing of pages.
//!
//! Each open page has a [`Room`]: a Yjs document (yrs) that connected editors keep in sync over a
//! WebSocket using the standard y-websocket protocol, plus a relay for cursor and presence state. The
//! stored form of a page stays its blocks: a few seconds after the last edit the room converts its
//! document to blocks ([`convert`]) and saves them through the same path as every other edit, so history,
//! links, search and export keep working unchanged. The document itself is also kept in the database
//! (`PageDoc`) so a restart does not lose what open tabs still hold.
//!
//! The hub's shape (a broadcast group per document, connections speaking the sync protocol) follows
//! `yrs-warp` (MIT, y-crdt), ported to axum and the current `yrs` and extended with per-user edit
//! attribution, view-only connections, and shutting a room down.

pub mod convert;

use crate::db::Db;
use crate::db::prelude::*;
use crate::models::{page_docs, users, wiki_pages};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Notify, broadcast, watch};
use yrs::block::ClientID;
use yrs::encoding::write::Write as _;
use yrs::sync::awareness::AwarenessUpdateEntry;
use yrs::sync::protocol::{MSG_SYNC, MSG_SYNC_UPDATE};
use yrs::sync::{AwarenessUpdate, Message, SyncMessage};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{Doc, Origin, ReadTxn, StateVector, Transact, Update};

use crate::blocks::validate_blocks;
use crate::error::ApiError;
use crate::state::AppState;

/// Close codes sent to clients (the 4000s are application defined).
pub mod close {
    /// The document was rebuilt (a save outside live editing, or a restore): reload the page.
    pub const RESET: u16 = 4001;
    /// The page was deleted.
    pub const DELETED: u16 = 4404;
    /// The person no longer has access.
    pub const FORBIDDEN: u16 = 4403;
}

/// How long after the last edit the document is saved as blocks.
const SAVE_AFTER_QUIET: Duration = Duration::from_secs(2);
/// An empty room is kept this long before it is saved and dropped.
const LINGER: Duration = Duration::from_secs(60);

/// A live document and the connections sharing it.
pub struct Room {
    pub page_id: uuid::Uuid,
    /// The lineage of the document; clients must present the one they were given.
    pub epoch: String,
    doc: Doc,
    /// Everything sent to every connection: document updates and presence changes.
    frames: broadcast::Sender<Vec<u8>>,
    /// Cursor and presence state by client, relayed between connections.
    presence: StdMutex<HashMap<ClientID, AwarenessUpdateEntry>>,
    /// Someone changed the document; save it soon.
    dirty: Notify,
    /// Who made the latest change not yet saved.
    last_editor: StdMutex<Option<uuid::Uuid>>,
    connections: AtomicUsize,
    /// Set when the room is shut down; connections then close with the code.
    shutdown: watch::Sender<Option<u16>>,
    idle_since: StdMutex<Instant>,
}

impl Room {
    fn new(page_id: uuid::Uuid, epoch: String, doc: Doc) -> Arc<Self> {
        let (frames, _) = broadcast::channel(256);
        let (shutdown, _) = watch::channel(None);
        Arc::new_cyclic(|weak: &std::sync::Weak<Room>| {
            let weak = weak.clone();
            let sender = frames.clone();
            doc.observe_update_v1("room", move |txn, event| {
                // Every change, from any connection, goes to every connection.
                let mut encoder = EncoderV1::new();
                encoder.write_var(MSG_SYNC);
                encoder.write_var(MSG_SYNC_UPDATE);
                encoder.write_buf(&event.update);
                let _ = sender.send(encoder.to_vec());
                if let Some(room) = weak.upgrade() {
                    // Changes carry who made them (set when applying a connection's update).
                    if let Some(origin) = txn.origin()
                        && let Ok(bytes) = <[u8; 16]>::try_from(origin.as_ref())
                    {
                        *room.last_editor.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(uuid::Uuid::from_bytes(bytes));
                    }
                    room.dirty.notify_one();
                }
            })
            .expect("observing a new document cannot fail");
            Room {
                page_id,
                epoch,
                doc,
                frames,
                presence: StdMutex::new(HashMap::new()),
                dirty: Notify::new(),
                last_editor: StdMutex::new(None),
                connections: AtomicUsize::new(0),
                shutdown,
                idle_since: StdMutex::new(Instant::now()),
            }
        })
    }

    /// Tell every connection to close with `code`, and stop the room's background work.
    fn shut_down(&self, code: u16) {
        let _ = self.shutdown.send(Some(code));
        self.dirty.notify_waiters();
    }

    fn is_shut_down(&self) -> bool {
        self.shutdown.borrow().is_some()
    }

    /// The current presence of everyone connected, as one awareness message.
    fn presence_message(&self) -> Option<Vec<u8>> {
        let clients = self
            .presence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        (!clients.is_empty()).then(|| Message::Awareness(AwarenessUpdate { clients }).encode_v1())
    }

    /// Apply a presence update from a connection: keep the newest state per client and pass it on.
    fn relay_presence(&self, update: AwarenessUpdate, seen: &mut Vec<ClientID>) {
        let mut changed = HashMap::new();
        {
            let mut states = self.presence.lock().unwrap_or_else(|e| e.into_inner());
            for (client, entry) in update.clients {
                if !seen.contains(&client) {
                    seen.push(client);
                }
                let newer = states
                    .get(&client)
                    .is_none_or(|old| entry.clock >= old.clock);
                if !newer {
                    continue;
                }
                if &*entry.json == "null" {
                    states.remove(&client);
                } else {
                    states.insert(client, entry.clone());
                }
                changed.insert(client, entry);
            }
        }
        if !changed.is_empty() {
            let _ = self
                .frames
                .send(Message::Awareness(AwarenessUpdate { clients: changed }).encode_v1());
        }
    }

    /// Remove the presence of clients that went away with their connection.
    fn drop_presence(&self, clients: &[ClientID]) {
        let mut gone = HashMap::new();
        {
            let mut states = self.presence.lock().unwrap_or_else(|e| e.into_inner());
            for client in clients {
                if let Some(old) = states.remove(client) {
                    gone.insert(
                        *client,
                        AwarenessUpdateEntry {
                            clock: old.clock + 1,
                            json: "null".into(),
                        },
                    );
                }
            }
        }
        if !gone.is_empty() {
            let _ = self
                .frames
                .send(Message::Awareness(AwarenessUpdate { clients: gone }).encode_v1());
        }
    }
}

/// What to do with one message from a connection; `reply` goes back to that connection only.
fn handle_message(
    room: &Room,
    user_id: uuid::Uuid,
    can_edit: bool,
    seen: &mut Vec<ClientID>,
    bytes: &[u8],
) -> Option<Vec<u8>> {
    let message = match Message::decode_v1(bytes) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!(%e, "undecodable live message");
            return None;
        }
    };
    match message {
        Message::Sync(SyncMessage::SyncStep1(state_vector)) => {
            // Everything the other side is missing.
            let diff = room.doc.transact().encode_state_as_update_v1(&state_vector);
            Some(Message::Sync(SyncMessage::SyncStep2(diff)).encode_v1())
        }
        Message::Sync(SyncMessage::SyncStep2(update) | SyncMessage::Update(update)) => {
            if !can_edit {
                return None;
            }
            match Update::decode_v1(&update) {
                Ok(update) => {
                    let origin = Origin::from(&user_id.as_bytes()[..]);
                    if let Err(e) = room.doc.transact_mut_with(origin).apply_update(update) {
                        tracing::debug!(%e, "rejected a live update");
                    }
                }
                Err(e) => tracing::debug!(%e, "undecodable live update"),
            }
            None
        }
        Message::Awareness(update) => {
            room.relay_presence(update, seen);
            None
        }
        Message::AwarenessQuery => room.presence_message(),
        Message::Auth(_) | Message::Custom(_, _) => None,
    }
}

/// Why a connection could not be added to a room.
pub enum JoinError {
    /// The client's document belongs to an older lineage; it should reload.
    StaleEpoch,
    Failed(ApiError),
}

/// The live rooms of this process.
#[derive(Default)]
pub struct Rooms {
    map: Mutex<HashMap<uuid::Uuid, Arc<Room>>>,
}

impl Rooms {
    /// The room for a page, loading or creating its document if nobody has it open.
    pub async fn room(&self, state: &AppState, page_id: uuid::Uuid) -> Result<Arc<Room>, ApiError> {
        let mut map = self.map.lock().await;
        if let Some(room) = map.get(&page_id).filter(|r| !r.is_shut_down()) {
            return Ok(room.clone());
        }
        let room = open_room(state, page_id).await?;
        map.insert(page_id, room.clone());
        tokio::spawn(maintain(state.clone(), room.clone()));
        Ok(room)
    }

    /// Throw the page's document away (its blocks were replaced outside live editing) and tell every
    /// editor to reload. Edits from the last few seconds that were not saved yet are lost.
    pub async fn reset(&self, state: &AppState, page_id: uuid::Uuid) {
        let room = self.map.lock().await.remove(&page_id);
        if let Some(room) = room {
            room.shut_down(close::RESET);
        }
        let Ok(db) = state.writer().await else {
            return;
        };
        if page_docs::Entity::delete_many()
            .filter(page_docs::Column::PageId.eq(page_id))
            .exec(&*db)
            .await
            .is_err()
        {
            tracing::warn!("could not discard a page's live document");
        }
    }

    /// Save every open room now (the server is shutting down), so the last seconds of typing are kept.
    pub async fn save_all(&self, state: &AppState) {
        let rooms: Vec<Arc<Room>> = self.map.lock().await.values().cloned().collect();
        for room in rooms {
            persist(state, &room).await;
        }
    }

    /// Close the page's room (it was deleted, or access ended) without saving it.
    pub async fn close(&self, page_id: uuid::Uuid, code: u16) {
        if let Some(room) = self.map.lock().await.remove(&page_id) {
            room.shut_down(code);
        }
    }
}

/// Load a page's document from the database, or build it from the page's blocks.
async fn open_room(state: &AppState, page_id: uuid::Uuid) -> Result<Arc<Room>, ApiError> {
    use base64::Engine;
    let writer = state.writer().await?;
    let db = &*writer;
    let page = wiki_pages::Entity::find_by_id(page_id)
        .one_or_not_found(db)
        .await?;
    let blocks = {
        let mut blocks = crate::pages::blocks_of(&page)?;
        crate::blocks::trim_trailing_empty(&mut blocks);
        blocks
    };
    if let Some(stored) = page_docs::Entity::find()
        .filter(page_docs::Column::PageId.eq(page_id))
        .one(db)
        .await?
    {
        let doc = Doc::new();
        let restored = base64::engine::general_purpose::STANDARD
            .decode(&stored.state)
            .ok()
            .and_then(|bytes| Update::decode_v1(&bytes).ok())
            .is_some_and(|update| doc.transact_mut().apply_update(update).is_ok());
        // Only trust the stored document while it still says what the page says.
        if restored && convert::blocks_from_doc(&doc) == blocks {
            return Ok(Room::new(page_id, stored.epoch, doc));
        }
    }
    let doc = Doc::new();
    convert::seed_doc(&doc, &blocks);
    let epoch = uuid::Uuid::new_v4().to_string();
    store_doc(db, page_id, &epoch, &doc).await?;
    Ok(Room::new(page_id, epoch, doc))
}

async fn store_doc(
    db: &impl Db,
    page_id: uuid::Uuid,
    epoch: &str,
    doc: &Doc,
) -> Result<(), ApiError> {
    use base64::Engine;
    let state = base64::engine::general_purpose::STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    let now = crate::time::now();
    match page_docs::Entity::find()
        .filter(page_docs::Column::PageId.eq(page_id))
        .one(db)
        .await?
    {
        Some(row) => {
            page_docs::ActiveModel {
                page_id: Unchanged(row.page_id),
                epoch: Set(epoch.to_string()),
                state: Set(state),
                updated_at: Set(now),
            }
            .update(db)
            .await?;
        }
        None => {
            page_docs::ActiveModel {
                page_id: Set(page_id),
                epoch: Set(epoch.to_string()),
                state: Set(state),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// Save the room's document as the page's blocks, if it changed.
async fn persist(state: &AppState, room: &Room) {
    let blocks = convert::blocks_from_doc(&room.doc);
    let editor = room
        .last_editor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let Ok(writer) = state.writer().await else {
        return;
    };
    let db = &*writer;
    let result: Result<(), ApiError> = async {
        let Ok(mut page) = wiki_pages::Entity::find_by_id(room.page_id)
            .one_or_not_found(&*db)
            .await
        else {
            return Ok(());
        };
        if page.deleted_at.is_some() || room.is_shut_down() {
            return Ok(());
        }
        if let Err(_e) = validate_blocks(&blocks) {
            tracing::warn!(page = %room.page_id, "live content is not valid as blocks; not saved");
            return Ok(());
        }
        let editor = editor.unwrap_or(page.updated_by);
        let tx = db.begin().await?;
        crate::pages::save_content(&tx, &mut page, editor, &blocks, false).await?;
        store_doc(&tx, room.page_id, &room.epoch, &room.doc).await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if result.is_err() {
        tracing::warn!(page = %room.page_id, "saving a live page failed");
    }
}

/// The room's background work: save after edits go quiet, and retire the room once it has been empty
/// for a while.
async fn maintain(state: AppState, room: Arc<Room>) {
    let mut shutdown = room.shutdown.subscribe();
    loop {
        tokio::select! {
            _ = room.dirty.notified() => {
                // Wait for a quiet spell; every further edit restarts the wait.
                while tokio::time::timeout(SAVE_AFTER_QUIET, room.dirty.notified()).await.is_ok() {}
                if room.is_shut_down() {
                    return;
                }
                persist(&state, &room).await;
            }
            _ = tokio::time::sleep(LINGER / 2) => {
                let idle = room.connections.load(Ordering::SeqCst) == 0
                    && room.idle_since.lock().unwrap_or_else(|e| e.into_inner()).elapsed() >= LINGER;
                if idle {
                    let mut map = state.rooms.map.lock().await;
                    if room.connections.load(Ordering::SeqCst) == 0 {
                        if map.get(&room.page_id).is_some_and(|r| Arc::ptr_eq(r, &room)) {
                            map.remove(&room.page_id);
                        }
                        drop(map);
                        persist(&state, &room).await;
                        return;
                    }
                }
            }
            _ = shutdown.changed() => return,
        }
    }
}

/// Serve one connection until it ends. `can_edit` false makes it view-only.
pub async fn serve(
    socket: axum::extract::ws::WebSocket,
    state: AppState,
    room: Arc<Room>,
    user_id: uuid::Uuid,
    mut can_edit: bool,
) {
    use axum::extract::ws::{CloseFrame, Message as Ws};
    use futures_util::{SinkExt, StreamExt};

    room.connections.fetch_add(1, Ordering::SeqCst);
    let (mut sink, mut stream) = socket.split();
    let mut frames = room.frames.subscribe();
    let mut shutdown = room.shutdown.subscribe();
    let mut seen: Vec<ClientID> = vec![];
    let mut access_check = tokio::time::interval(Duration::from_secs(60));
    access_check.tick().await;

    // Greet like a y-websocket server: our state vector (so the client sends what we lack) and who else is here.
    let hello =
        Message::Sync(SyncMessage::SyncStep1(room.doc.transact().state_vector())).encode_v1();
    let mut open = sink.send(Ws::Binary(hello.into())).await.is_ok();
    if open && let Some(presence) = room.presence_message() {
        open = sink.send(Ws::Binary(presence.into())).await.is_ok();
    }

    let mut close_with: Option<u16> = None;
    // The loop ends (`break`) when the connection, the room or the person's access does.
    #[allow(clippy::while_immutable_condition)]
    while open {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Ws::Binary(bytes))) => {
                    if let Some(reply) = handle_message(&room, user_id, can_edit, &mut seen, &bytes)
                        && sink.send(Ws::Binary(reply.into())).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Ws::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            frame = frames.recv() => match frame {
                Ok(bytes) => {
                    if sink.send(Ws::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                // Fell behind: the client resyncs when it reconnects.
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = shutdown.changed() => {
                close_with = *shutdown.borrow();
                break;
            }
            _ = access_check.tick() => {
                let db = state.db();
                let page_ok = wiki_pages::Entity::find_by_id(room.page_id).one_or_not_found(db).await.is_ok_and(|p| p.deleted_at.is_none());
                let Ok(user) = users::Entity::find_by_id(user_id).one_or_not_found(db).await else { continue };
                let workspace = state_workspace(db, room.page_id).await;
                match crate::access::role_in(db, &user, workspace).await {
                    Ok(Some(role)) if page_ok && user.active => can_edit = role >= crate::models::Role::Write,
                    _ => {
                        close_with = Some(if page_ok { close::FORBIDDEN } else { close::DELETED });
                        break;
                    }
                }
            }
        }
    }

    room.drop_presence(&seen);
    if let Some(code) = close_with {
        let _ = sink
            .send(Ws::Close(Some(CloseFrame {
                code,
                reason: "".into(),
            })))
            .await;
    }
    let _ = sink.close().await;
    *room.idle_since.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    if room.connections.fetch_sub(1, Ordering::SeqCst) == 1 && !room.is_shut_down() {
        // The last editor left: save now rather than waiting for the quiet period.
        room.dirty.notify_one();
    }
}

async fn state_workspace(db: &impl Db, page_id: uuid::Uuid) -> uuid::Uuid {
    wiki_pages::Entity::find_by_id(page_id)
        .one_or_not_found(db)
        .await
        .map(|p| p.workspace_id)
        .unwrap_or_default()
}
