//! What travels on the wire, and what gets written down.
use serde::{Deserialize, Serialize};

pub const KIND_CHAT: &str = "chat";
pub const KIND_CHANGE: &str = "change";
pub const KIND_FILE: &str = "file";

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
    /// Which machine it came from. `from` is a display name an AI may choose
    /// for itself, so without this you could not tell whose Claude spoke —
    /// which is the one thing this tool exists to answer.
    #[serde(default, skip_serializing_if = "empty")]
    pub host: String,
    #[serde(default)]
    pub text: String,
    /// Who this is aimed at, from the @names in the text. Empty means everyone.
    /// It narrows who is *told*, never who can read it: the channel is a shared
    /// record, and a mention that hid the message would put private side-talk
    /// inside something two people rely on being complete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,

    /// file only: what was sent, by name, size and hash. The bytes are not
    /// here — they are in the store, fetched when somebody actually wants them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<crate::files::FileRef>,

    // change only
    #[serde(default, skip_serializing_if = "empty")]
    pub action: String,
    #[serde(default, skip_serializing_if = "empty")]
    pub target: String,
}

impl Msg {
    pub fn kind(&self) -> &str {
        match self.kind.as_str() {
            KIND_CHANGE => KIND_CHANGE,
            KIND_FILE => KIND_FILE,
            _ => KIND_CHAT,
        }
    }

    pub fn is_file(&self) -> bool {
        self.kind() == KIND_FILE
    }

    pub fn is_change(&self) -> bool {
        self.kind() == KIND_CHANGE
    }

    /// The name to show. An AI that has named itself is shown by that name; one
    /// that has not is "<machine>'s AI", which is what it was before.
    pub fn who(&self) -> String {
        if self.via != ACTOR_AI {
            return self.from.clone();
        }
        // Records written before names existed carry no host: back then `from`
        // was the machine, so that is what to say it belongs to.
        if self.host.is_empty() {
            return format!("{}'s AI", self.from);
        }
        if self.from.is_empty() || self.from == self.host {
            return format!("{}'s AI", self.host);
        }
        self.from.clone()
    }

    /// Name plus machine, for anywhere there is no room for two columns —
    /// a chat that named itself "shop" says nothing about whose Claude it is.
    pub fn label(&self) -> String {
        match self.machine() {
            Some(h) => format!("{} ({h})", self.who()),
            None => self.who(),
        }
    }

    /// Whether this message should interrupt somebody answering to any of
    /// these names. Unaddressed messages are for everyone.
    pub fn is_for(&self, names: &[String]) -> bool {
        if self.to.is_empty() {
            return true;
        }
        names
            .iter()
            .any(|n| self.to.iter().any(|t| t == &n.to_lowercase()))
    }

    /// The machine, when it is not already obvious from the name.
    pub fn machine(&self) -> Option<&str> {
        if self.host.is_empty() || self.who().contains(&self.host) {
            None
        } else {
            Some(&self.host)
        }
    }

    /// One line, for a terminal — this is what Monitor ends up showing.
    pub fn line(&self) -> String {
        if let Some(f) = self.file.as_ref().filter(|_| self.is_file()) {
            let caption = if self.text.is_empty() {
                String::new()
            } else {
                format!(" — {}", self.text)
            };
            return format!(
                "[file] {} ({}){caption}",
                f.name,
                crate::files::human(f.size)
            );
        }
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

/// Pulls @names out of a message. A name must follow a space or start the
/// line, so an email address does not read as three mentions.
pub fn mentions_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut in_code = false;
    let mut i = 0;
    while i < chars.len() {
        // Inside backticks you are quoting a name, not calling one. Without
        // this, the one message a channel cannot accept is the message
        // explaining why a name does not work on that channel.
        if chars[i] == '`' {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if chars[i] == '@' && !in_code {
            // A name must follow a space or start the line, so an email address
            // is an address; and @@name is how you write a literal one.
            let prev_ok = i == 0 || (!chars[i - 1].is_alphanumeric() && chars[i - 1] != '@');
            if prev_ok {
                let mut j = i + 1;
                let mut name = String::new();
                while j < chars.len()
                    && (chars[j].is_alphanumeric() || matches!(chars[j], '-' | '_' | '.' | '/'))
                {
                    name.push(chars[j]);
                    j += 1;
                }
                let name = name.trim_end_matches(['.', '/']).to_lowercase();
                if !name.is_empty() && !out.contains(&name) {
                    out.push(name);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn now() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .format(&Rfc3339)
        .unwrap_or_default()
}
