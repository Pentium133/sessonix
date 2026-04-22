//! Minimal Telegram Bot API client built on the already-vendored `ureq` HTTP
//! client. Covers only what the bridge needs: long-poll updates, send messages
//! (with reply-to), send documents, and post message reactions.
//!
//! Hand-rolled rather than pulled from `teloxide` so the codebase stays on
//! `std::thread` + `ureq` without introducing tokio and its ecosystem for
//! four API methods.

use serde::Deserialize;

/// Telegram's hard ceiling for a plain-text message body.
pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

/// Telegram's hard ceiling for a document/photo caption. Strictly smaller than
/// the message limit, so the attachment path must trim captions separately.
pub const TELEGRAM_CAPTION_LIMIT: usize = 1024;

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub from: Option<User>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reply_to_message: Option<Box<Message>>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

pub struct TelegramApi {
    token: String,
}

impl TelegramApi {
    pub fn new(token: String) -> Self {
        Self { token }
    }

    fn url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    /// Long-poll for updates. `offset` is `last_seen_update_id + 1` (or 0 for
    /// first call); `timeout_secs` is how long the server may hold the
    /// connection open before replying with an empty list. Returns the raw
    /// update list — caller advances its offset cursor.
    pub fn get_updates(&self, offset: i64, timeout_secs: u64) -> Result<Vec<Update>, String> {
        let url = self.url("getUpdates");
        // Allow the HTTP read timeout to exceed the long-poll timeout a bit so
        // the server's own "timeout" response has room to arrive.
        let mut resp = ureq::get(&url)
            .query("offset", offset.to_string())
            .query("timeout", timeout_secs.to_string())
            // Limit the shape of messages we deserialize.
            .query("allowed_updates", r#"["message"]"#)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(timeout_secs + 5)))
            .build()
            .call()
            .map_err(|e| format!("getUpdates: {e}"))?;

        let body: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("getUpdates parse: {e}"))?;

        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(format!(
                "getUpdates not ok: {}",
                body.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            ));
        }

        let result = body.get("result").cloned().unwrap_or(serde_json::Value::Null);
        serde_json::from_value::<Vec<Update>>(result)
            .map_err(|e| format!("getUpdates deserialize: {e}"))
    }

    /// Send a plain-text message. When `reply_to` is `Some`, the message is
    /// posted as a reply to that message id in the same chat. Returns the
    /// sent message's `message_id` on success.
    pub fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
    ) -> Result<i64, String> {
        self.send_message_opts(chat_id, text, reply_to, None)
    }

    /// Same as [`send_message`] but accepts a `parse_mode` (`"HTML"` or
    /// `"MarkdownV2"`). Used for Idle notifications where we convert the
    /// agent's markdown reply into Telegram HTML so code blocks and bold
    /// render natively.
    pub fn send_message_opts(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        parse_mode: Option<&str>,
    ) -> Result<i64, String> {
        let url = self.url("sendMessage");
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(mid) = reply_to {
            payload["reply_to_message_id"] = mid.into();
        }
        if let Some(mode) = parse_mode {
            payload["parse_mode"] = mode.into();
        }

        let mut resp = ureq::post(&url)
            .send_json(&payload)
            .map_err(|e| format!("sendMessage: {e}"))?;
        let body: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("sendMessage parse: {e}"))?;

        // If HTML parsing is rejected (unbalanced tag etc.), retry once in
        // plain text so the user doesn't get silence on a formatting glitch.
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
            && parse_mode.is_some()
        {
            log::warn!(
                "sendMessage HTML rejected ({}); retrying without parse_mode",
                body.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            );
            return self.send_message_opts(chat_id, text, reply_to, None);
        }

        extract_sent_message_id(&body)
    }

    /// Send a UTF-8 document (e.g. a markdown transcript) as an attachment,
    /// optionally with a text caption. Returns the sent message's id on
    /// success. `filename` is what Telegram shows as the download name.
    ///
    /// Caption is always submitted with `parse_mode=HTML` — our captions are
    /// produced by the markdown→HTML converter and would otherwise render
    /// their tags as literal text.
    pub fn send_document(
        &self,
        chat_id: i64,
        filename: &str,
        contents: &[u8],
        caption: Option<&str>,
        reply_to: Option<i64>,
    ) -> Result<i64, String> {
        let url = self.url("sendDocument");
        let boundary = format!("----sessonix-{}", generate_boundary());

        let mut body: Vec<u8> = Vec::with_capacity(contents.len() + 512);
        multipart_field(&mut body, &boundary, "chat_id", chat_id.to_string().as_bytes());
        if let Some(cap) = caption {
            multipart_field(&mut body, &boundary, "caption", cap.as_bytes());
            multipart_field(&mut body, &boundary, "parse_mode", b"HTML");
        }
        if let Some(mid) = reply_to {
            multipart_field(
                &mut body,
                &boundary,
                "reply_to_message_id",
                mid.to_string().as_bytes(),
            );
        }
        multipart_file(&mut body, &boundary, "document", filename, contents);
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let mut resp = ureq::post(&url)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}").as_str(),
            )
            .send(body.as_slice())
            .map_err(|e| format!("sendDocument: {e}"))?;

        let parsed: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("sendDocument parse: {e}"))?;

        extract_sent_message_id(&parsed)
    }

    /// Post a single emoji reaction to a message. Errors are swallowed by the
    /// caller — reactions are cosmetic and must not block the send path.
    pub fn set_reaction(&self, chat_id: i64, message_id: i64, emoji: &str) -> Result<(), String> {
        let url = self.url("setMessageReaction");
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": [{"type": "emoji", "emoji": emoji}],
        });
        ureq::post(&url)
            .send_json(&payload)
            .map_err(|e| format!("setMessageReaction: {e}"))?;
        Ok(())
    }

    /// GET /getMe — quickest sanity check that the token is valid and the
    /// network path is working. Returns the bot's `id` on success.
    pub fn get_me(&self) -> Result<i64, String> {
        let url = self.url("getMe");
        let mut resp = ureq::get(&url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build()
            .call()
            .map_err(|e| format!("getMe: {e}"))?;
        let body: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("getMe parse: {e}"))?;
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(format!(
                "getMe not ok: {}",
                body.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            ));
        }
        body.get("result")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "getMe missing id".to_string())
    }
}

fn extract_sent_message_id(body: &serde_json::Value) -> Result<i64, String> {
    if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(format!(
            "telegram not ok: {}",
            body.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ));
    }
    body.get("result")
        .and_then(|r| r.get("message_id"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "missing message_id".to_string())
}

fn generate_boundary() -> String {
    // Non-cryptographic uniqueness: nanos + process id is enough.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), nanos)
}

fn multipart_field(buf: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
    buf.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    buf.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    buf.extend_from_slice(value);
    buf.extend_from_slice(b"\r\n");
}

fn multipart_file(
    buf: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    contents: &[u8],
) {
    buf.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    buf.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
        )
        .as_bytes(),
    );
    buf.extend_from_slice(b"Content-Type: text/markdown; charset=utf-8\r\n\r\n");
    buf.extend_from_slice(contents);
    buf.extend_from_slice(b"\r\n");
}
