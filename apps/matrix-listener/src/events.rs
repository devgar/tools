//! Pure extraction of the messages to dispatch from a `/sync` response.

use serde_json::{Value, json};

/// Messages from joined rooms not sent by the bot, with `room_id` and `body`
/// added to each event so subscribers get the same shape as with the Python
/// `element_listener`.
pub fn extract_messages(sync: &Value, botname: &str) -> Vec<Value> {
    let Some(rooms) = sync["rooms"]["join"].as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (room_id, room) in rooms {
        let Some(events) = room["timeline"]["events"].as_array() else {
            continue;
        };
        for event in events {
            if event["type"] != "m.room.message" || event["sender"] == botname {
                continue;
            }
            let mut event = event.clone();
            let body = event["content"]["body"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            event["room_id"] = json!(room_id);
            event["body"] = json!(body);
            out.push(event);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "@bot:example.org";

    fn message(sender: &str, body: &str) -> Value {
        json!({
            "type": "m.room.message",
            "sender": sender,
            "event_id": format!("$ev-{body}"),
            "content": { "msgtype": "m.text", "body": body },
        })
    }

    fn sync_with(rooms: Value) -> Value {
        json!({ "next_batch": "s1", "rooms": { "join": rooms } })
    }

    #[test]
    fn adds_room_id_and_body() {
        let sync = sync_with(json!({
            "!a:example.org": { "timeline": { "events": [message("@u:example.org", "hi")] } }
        }));
        let out = extract_messages(&sync, BOT);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["room_id"], "!a:example.org");
        assert_eq!(out[0]["body"], "hi");
        assert_eq!(out[0]["event_id"], "$ev-hi");
        assert_eq!(out[0]["content"]["body"], "hi");
    }

    #[test]
    fn ignores_messages_from_the_bot() {
        let sync = sync_with(json!({
            "!a:example.org": { "timeline": { "events": [
                message(BOT, "Pong! 🏓"),
                message("@u:example.org", "!ping"),
            ] } }
        }));
        let out = extract_messages(&sync, BOT);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["sender"], "@u:example.org");
    }

    #[test]
    fn ignores_other_event_types() {
        let sync = sync_with(json!({
            "!a:example.org": { "timeline": { "events": [
                { "type": "m.reaction", "sender": "@u:example.org", "content": {} },
                { "type": "m.room.member", "sender": "@u:example.org", "content": {} },
            ] } }
        }));
        assert!(extract_messages(&sync, BOT).is_empty());
    }

    #[test]
    fn missing_body_becomes_empty_string() {
        let sync = sync_with(json!({
            "!a:example.org": { "timeline": { "events": [
                { "type": "m.room.message", "sender": "@u:example.org", "content": {} },
            ] } }
        }));
        let out = extract_messages(&sync, BOT);
        assert_eq!(out[0]["body"], "");
    }

    #[test]
    fn collects_from_every_joined_room() {
        let sync = sync_with(json!({
            "!a:example.org": { "timeline": { "events": [message("@u:example.org", "one")] } },
            "!b:example.org": { "timeline": { "events": [message("@u:example.org", "two")] } },
            "!c:example.org": {},
        }));
        let mut rooms: Vec<_> = extract_messages(&sync, BOT)
            .iter()
            .map(|e| e["room_id"].to_string())
            .collect();
        rooms.sort();
        assert_eq!(rooms, ["\"!a:example.org\"", "\"!b:example.org\""]);
    }

    #[test]
    fn empty_sync_yields_nothing() {
        assert!(extract_messages(&json!({ "next_batch": "s1" }), BOT).is_empty());
    }
}
