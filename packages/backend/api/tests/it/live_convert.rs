//! The server reads documents written by the real editor (fixtures produced by the frontend test
//! `yjsFixtures.test.ts`) and must get the same blocks the editor's own save produces.

use woodmouse::blocks::{BlockData, validate_blocks};
use woodmouse::live::convert::blocks_from_doc;
use yrs::updates::decoder::Decode;
use yrs::{Doc, Transact, Update};

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    update_base64: String,
    blocks: Vec<BlockData>,
}

#[test]
fn documents_written_by_the_editor_become_the_blocks_the_editor_saves() {
    use base64::Engine;
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/it/fixtures/editor_docs.json"
    ))
    .unwrap();
    let fixtures: Vec<Fixture> = serde_json::from_str(&text).unwrap();
    assert!(fixtures.len() >= 4);
    for fixture in fixtures {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&fixture.update_base64)
            .unwrap();
        let doc = Doc::new();
        doc.transact_mut()
            .apply_update(Update::decode_v1(&bytes).unwrap())
            .unwrap();
        let blocks = blocks_from_doc(&doc);
        assert_eq!(blocks, fixture.blocks, "{}", fixture.name);
        assert!(validate_blocks(&blocks).is_ok(), "{}", fixture.name);
    }
}
