//! Live editing over a real WebSocket: clients exchange Yjs updates through the room, and what they
//! write is saved as the page's blocks.

mod common;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as Wire;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use yrs::sync::{Message, SyncMessage};
use yrs::types::xml::{XmlFragment, XmlOut};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, Transact, WriteTxn, XmlTextPrelim};

use common::*;

struct Fx {
    app: axum::Router,
    addr: std::net::SocketAddr,
    cookie: String,
    ws: uuid::Uuid,
}

async fn fixture() -> Fx {
    let (app, _db) = app_with_db().await;
    let cookie = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &cookie, "Home").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    Fx {
        app,
        addr,
        cookie,
        ws,
    }
}

impl Fx {
    async fn json(&self, req: axum::http::Request<axum::body::Body>) -> Value {
        let res = request(&self.app, req).await;
        serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
    }
    async fn page(&self, title: &str) -> String {
        let body = json!({ "title": title }).to_string();
        let v = self
            .json(post_json(
                &format!("/api/workspaces/{}/pages", self.ws),
                &body,
                Some(&self.cookie),
            ))
            .await;
        v["id"].as_str().unwrap().to_string()
    }
    async fn session(&self, page: &str) -> String {
        let v = self
            .json(get(
                &format!("/api/pages/{page}/live-session"),
                Some(&self.cookie),
            ))
            .await;
        v["epoch"].as_str().unwrap().to_string()
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A test client: a Yjs document kept in sync with the room.
struct Client {
    doc: Doc,
    socket: Socket,
}

impl Client {
    async fn connect(fx: &Fx, page: &str, epoch: &str) -> Client {
        let url = format!("ws://{}/api/pages/{page}/live?epoch={epoch}", fx.addr);
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("cookie", fx.cookie.parse().unwrap());
        let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let mut client = Client {
            doc: Doc::new(),
            socket,
        };
        // Ask for the room's content, like a real provider does on open.
        let step1 = Message::Sync(SyncMessage::SyncStep1(client.doc.transact().state_vector()));
        client.send(step1.encode_v1()).await;
        client
    }

    async fn send(&mut self, bytes: Vec<u8>) {
        self.socket.send(Wire::Binary(bytes.into())).await.unwrap();
    }

    /// Read messages for a while, applying document updates and answering sync requests.
    async fn pump(&mut self, wait: Duration) -> Option<u16> {
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Some(msg)) = tokio::time::timeout_at(deadline, self.socket.next()).await {
            match msg.unwrap() {
                Wire::Binary(bytes) => match Message::decode_v1(&bytes).unwrap() {
                    Message::Sync(SyncMessage::SyncStep1(sv)) => {
                        let diff = self.doc.transact().encode_state_as_update_v1(&sv);
                        self.send(Message::Sync(SyncMessage::SyncStep2(diff)).encode_v1())
                            .await;
                    }
                    Message::Sync(SyncMessage::SyncStep2(u) | SyncMessage::Update(u)) => {
                        let update = yrs::Update::decode_v1(&u).unwrap();
                        self.doc.transact_mut().apply_update(update).unwrap();
                    }
                    _ => {}
                },
                Wire::Close(frame) => return frame.map(|f| u16::from(f.code)),
                _ => {}
            }
        }
        None
    }

    /// Type text at the end of the first paragraph and send the change.
    async fn type_text(&mut self, text: &str) {
        let before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment("default");
            let Some(XmlOut::Element(paragraph)) = fragment.get(&txn, 0) else {
                panic!("the room should have sent an empty paragraph");
            };
            paragraph.push_back(&mut txn, XmlTextPrelim::new(text));
        }
        let diff = self.doc.transact().encode_diff_v1(&before);
        self.send(Message::Sync(SyncMessage::Update(diff)).encode_v1())
            .await;
    }

    fn text(&self) -> String {
        let txn = self.doc.transact();
        txn.get_xml_fragment("default")
            .map(|f| f.get_string(&txn))
            .unwrap_or_default()
    }
}

#[tokio::test]
async fn edits_reach_other_editors_and_are_saved_as_blocks() {
    let fx = fixture().await;
    let page = fx.page("Notes").await;
    let epoch = fx.session(&page).await;

    let mut a = Client::connect(&fx, &page, &epoch).await;
    a.pump(Duration::from_millis(300)).await;
    a.type_text("hello live").await;

    // A second editor joining afterwards gets the content.
    let mut b = Client::connect(&fx, &page, &epoch).await;
    b.pump(Duration::from_millis(500)).await;
    assert!(b.text().contains("hello live"), "{}", b.text());

    // And what B adds reaches A.
    b.type_text(" and more").await;
    a.pump(Duration::from_millis(500)).await;
    assert!(a.text().contains("and more"), "{}", a.text());

    // After a quiet moment the page's blocks hold the text (saved by the room).
    let mut saved = Value::Null;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        saved = fx
            .json(get(&format!("/api/pages/{page}"), Some(&fx.cookie)))
            .await;
        if saved["blocks"][0]["spans"][0]["text"].is_string() {
            break;
        }
    }
    let text: String = saved["blocks"][0]["spans"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["text"].as_str())
        .collect();
    assert!(
        text.contains("hello live") && text.contains("and more"),
        "{text}"
    );
    assert_eq!(saved["updated_by"]["email"], "admin@home.io");
}

#[tokio::test]
async fn a_stale_epoch_or_a_save_outside_live_editing_closes_the_socket() {
    let fx = fixture().await;
    let page = fx.page("Notes").await;
    let epoch = fx.session(&page).await;

    // An old lineage is told to start over.
    let mut stale = Client::connect(&fx, &page, "not-the-epoch").await;
    assert_eq!(stale.pump(Duration::from_millis(500)).await, Some(4001));

    // A save through the REST endpoint rebuilds the document: connected editors reload.
    let mut a = Client::connect(&fx, &page, &epoch).await;
    a.pump(Duration::from_millis(300)).await;
    let body = json!({ "blocks": [{ "type": "paragraph", "spans": [{ "text": "from rest" }] }] });
    let res = request(
        &fx.app,
        put_json(
            &format!("/api/pages/{page}/blocks"),
            &body.to_string(),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert!(res.status().is_success());
    assert_eq!(a.pump(Duration::from_secs(2)).await, Some(4001));

    // The next session has a new epoch and starts from the saved blocks.
    let fresh = fx.session(&page).await;
    assert_ne!(fresh, epoch);
    let mut b = Client::connect(&fx, &page, &fresh).await;
    b.pump(Duration::from_millis(500)).await;
    assert!(b.text().contains("from rest"), "{}", b.text());
}
