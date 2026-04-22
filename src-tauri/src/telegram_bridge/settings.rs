//! Thin wrappers over the generic `settings` key-value store for the two
//! pieces of persistent state the Telegram bridge needs: the bot token and
//! the claimed owner `chat_id`. Kept as tiny functions rather than a struct
//! so callers can mix and match without threading a helper around.

use crate::db::Db;
use rusqlite::Error;

const TOKEN_KEY: &str = "telegram_bot_token";
const OWNER_KEY: &str = "telegram_owner_chat_id";

pub fn get_token(db: &Db) -> Result<Option<String>, Error> {
    db.get_setting(TOKEN_KEY)
        .map(|v| v.filter(|s| !s.is_empty()))
}

pub fn set_token(db: &Db, token: Option<&str>) -> Result<(), Error> {
    match token {
        Some(t) => db.set_setting(TOKEN_KEY, t),
        // "Clear" is a zero-length write; readers treat empty as absent.
        None => db.set_setting(TOKEN_KEY, ""),
    }
}

pub fn get_owner(db: &Db) -> Result<Option<i64>, Error> {
    let Some(raw) = db.get_setting(OWNER_KEY)? else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    Ok(raw.parse::<i64>().ok())
}

pub fn set_owner(db: &Db, chat_id: i64) -> Result<(), Error> {
    db.set_setting(OWNER_KEY, &chat_id.to_string())
}

pub fn clear_owner(db: &Db) -> Result<(), Error> {
    db.set_setting(OWNER_KEY, "")
}
