//! What travels on the wire, and what gets written down.
use serde::{Deserialize, Serialize};

pub const KIND_CHAT: &str = "chat";
pub const KIND_CHANGE: &str = "change";

/// Who actually spoke. A name in collab belongs to a machine, not a person, so
/// without this "sis" means both the other person and their Claude — and "sis is asking
/// you something" deserves a different reaction from "sis's AI edited a script".
pub const ACTOR_AI: &str = "ai";

/// What a change did. Anything else is rejected at the door.
pub const ACTIONS: [&str; 4] = ["added", "edited", "removed", "renamed"];

// serde hands skip_serializing_if a &String, so &str is not an option here.
#[allow(clippy::ptr_arg)]
fn empty(s: &String) -> bool {
    s.is_empty()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Msg {
    #[serde(default)]
    pub seq: i64,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub at: String,
    /// "" (records written by older versions) means chat.
    #[serde(default, skip_serializing_if = "empty")]
    pub kind: String,
    /// "ai", or "" for a person.
    #[serde(default, skip_serializing_if = "empty")]
    pub via: String,
    #[serde(default)]
    pub text: String,
    // change only
    #[serde(default, skip_serializing_if = "empty")]
    pub action: String,
    #[serde(default, skip_serializing_if = "empty")]
    pub target: String,
}

impl Msg {
    pub fn kind(&self) -> &str {
        if self.kind == KIND_CHANGE {
            KIND_CHANGE
        } else {
            KIND_CHAT
        }
    }

    pub fn is_change(&self) -> bool {
        self.kind() == KIND_CHANGE
    }

    /// The name to show: the machine's, or its AI's.
    pub fn who(&self) -> String {
        if self.via == ACTOR_AI {
            format!("{}'s AI", self.from)
        } else {
            self.from.clone()
        }
    }

    /// One line, for a terminal — this is what Monitor ends up showing.
    pub fn line(&self) -> String {
        if self.is_change() {
            if !self.target.is_empty() {
                return format!("[{}] {} — {}", self.action, self.target, self.text);
            }
            return format!("[{}] {}", self.action, self.text);
        }
        self.text.clone()
    }

    /// HH:MM out of an RFC3339 stamp, without pulling in date parsing.
    pub fn hhmm(&self) -> &str {
        if self.at.len() >= 16 {
            &self.at[11..16]
        } else {
            "--:--"
        }
    }
}

pub fn now() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .format(&Rfc3339)
        .unwrap_or_default()
}
