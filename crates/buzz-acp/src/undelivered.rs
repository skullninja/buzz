//! Reporting turns whose text never reached the channel.
//!
//! buzz-acp does not publish replies — an agent sends its own through the CLI —
//! so a turn can finish cleanly having delivered nothing, with no error
//! anywhere. This asks the relay whether anything actually landed, and reports
//! it when nothing did.
//!
//! Kept in its own module rather than inline in `lib.rs`: that file is large
//! and changes upstream constantly, and every line we add there is a place a
//! future rebase can conflict. The call site is three lines.

use crate::{observer, relay};

/// Characters of assistant text kept per turn.
///
/// Enough to recognise what was said; small enough that buzz-acp does not
/// start retaining message content, which it otherwise deliberately does not.
const TURN_TEXT_HEAD_CHARS: usize = 200;

/// What an agent said during a turn, for reporting a delivery failure.
#[derive(Debug, Clone)]
pub struct TurnText {
    /// The first [`TURN_TEXT_HEAD_CHARS`] characters, never split mid-character.
    pub head: String,
    /// Total characters produced, however much of it was kept.
    pub total_chars: usize,
    /// When the turn began, as unix seconds — the lower bound for asking the
    /// relay whether anything the agent said actually landed.
    pub started_at_unix: u64,
}

/// Accumulates a bounded preview of one turn's assistant text.
///
/// Lives here rather than on `AcpClient` so the client keeps a single field
/// and the logic sits in a file we own — `acp.rs` changes upstream constantly.
#[derive(Debug)]
pub(crate) struct TurnTextBuffer {
    head: String,
    chars: usize,
    started_at: std::time::SystemTime,
}

impl Default for TurnTextBuffer {
    fn default() -> Self {
        Self {
            head: String::new(),
            chars: 0,
            started_at: std::time::SystemTime::now(),
        }
    }
}

impl TurnTextBuffer {
    /// Start a new turn. A turn must never inherit the previous turn's text.
    pub(crate) fn begin(&mut self) {
        self.head.clear();
        self.chars = 0;
        self.started_at = std::time::SystemTime::now();
    }

    /// Record a chunk of assistant text produced during the current turn.
    pub(crate) fn record(&mut self, text: &str) {
        self.chars += text.chars().count();

        let remaining = TURN_TEXT_HEAD_CHARS.saturating_sub(self.head.chars().count());
        if remaining > 0 {
            // Take by character, not by byte: a byte slice would split
            // multi-byte characters and produce invalid output.
            self.head.extend(text.chars().take(remaining));
        }
    }

    /// Take what the agent said this turn, clearing it for the next.
    ///
    /// `None` when the turn produced no text at all — a legitimate outcome,
    /// not a failure.
    pub(crate) fn take(&mut self) -> Option<TurnText> {
        if self.chars == 0 {
            return None;
        }
        Some(TurnText {
            head: std::mem::take(&mut self.head),
            total_chars: std::mem::replace(&mut self.chars, 0),
            started_at_unix: self
                .started_at
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        })
    }
}

/// Whether a completed turn looks like a silent delivery failure.
///
/// Only *undelivered speech* is a defect. A turn that said nothing is
/// frequently correct — Buzz's own base prompt tells agents that silence is
/// usually the right answer — so it must never be flagged.
pub(crate) fn is_silent_delivery(text: Option<&TurnText>, published: u64) -> bool {
    text.is_some() && published == 0
}

/// How long to wait before asking the relay whether the agent's reply landed.
///
/// The agent publishes its own message through the CLI, so that write races
/// this check. Long enough for it to land, short enough that the warning still
/// refers to the turn the operator just watched.
const UNPUBLISHED_CHECK_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Check in the background whether a turn's text reached the channel, and
/// report to the observer feed when it did not.
///
/// Spawned because the caller is synchronous, and because giving the agent's
/// own publish a moment to land is correct rather than merely convenient.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_check(
    text: TurnText,
    author: nostr::PublicKey,
    channel: uuid::Uuid,
    agent_index: usize,
    turn_id: String,
    observer: observer::ObserverHandle,
    rest: relay::RestClient,
) {
    tokio::spawn(async move {
        tokio::time::sleep(UNPUBLISHED_CHECK_GRACE).await;

        // Both message kinds: an agent publishing v2 must not look silent.
        let filter = nostr::Filter::new()
            .author(author)
            .kinds([
                nostr::Kind::Custom(buzz_core::kind::KIND_STREAM_MESSAGE as u16),
                nostr::Kind::Custom(buzz_core::kind::KIND_STREAM_MESSAGE_V2 as u16),
            ])
            .since(nostr::Timestamp::from(text.started_at_unix))
            .custom_tags(
                nostr::SingleLetterTag::lowercase(nostr::Alphabet::H),
                [channel.to_string()],
            );

        let published = match rest.count(&[filter]).await {
            Ok(value) => value.get("count").and_then(|c| c.as_u64()).unwrap_or(1),
            // A failed count assumes published. A false alarm is worse than a
            // missed one: it teaches the operator to ignore the warning, and
            // then it is worth nothing.
            Err(error) => {
                tracing::debug!("undelivered-turn check could not count: {error}");
                1
            }
        };

        if is_silent_delivery(Some(&text), published) {
            tracing::warn!(
                agent = agent_index,
                chars = text.total_chars,
                "turn produced text but published no message"
            );
            observer.emit(
                "turn_error",
                Some(agent_index),
                &observer::context_for(Some(channel), None, Some(turn_id)),
                serde_json::json!({
                    "outcome": "unpublished",
                    "error": format!(
                        "The turn produced {} characters and published no message to this \
                         channel. Nothing was sent. The text began: {}",
                        text.total_chars, text.head
                    ),
                }),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some_text() -> TurnText {
        TurnText {
            head: "hello".into(),
            total_chars: 5,
            started_at_unix: 0,
        }
    }

    #[test]
    fn text_with_no_published_message_is_a_silent_failure() {
        assert!(is_silent_delivery(Some(&some_text()), 0));
    }

    #[test]
    fn text_that_was_published_is_not_a_failure() {
        assert!(!is_silent_delivery(Some(&some_text()), 1));
    }

    #[test]
    fn a_turn_that_said_nothing_is_not_a_failure() {
        // Silence is frequently correct; only undelivered speech is a defect.
        assert!(!is_silent_delivery(None, 0));
    }

    #[test]
    fn turn_text_keeps_a_bounded_head_and_a_full_count() {
        let mut buffer = TurnTextBuffer::default();
        for _ in 0..50 {
            buffer.record("0123456789");
        }

        let text = buffer.take().expect("text was produced");
        assert_eq!(text.total_chars, 500, "the full length is reported");
        assert!(
            text.head.chars().count() <= TURN_TEXT_HEAD_CHARS,
            "the head is capped so a long turn cannot grow a buffer"
        );
    }

    #[test]
    fn turn_text_is_taken_once_and_reset() {
        let mut buffer = TurnTextBuffer::default();
        buffer.record("something");
        assert!(buffer.take().is_some());
        assert!(
            buffer.take().is_none(),
            "a second take must not repeat the previous turn"
        );
    }

    #[test]
    fn a_turn_with_no_text_yields_nothing() {
        assert!(TurnTextBuffer::default().take().is_none());
    }

    #[test]
    fn multibyte_text_is_not_split_mid_character() {
        let mut buffer = TurnTextBuffer::default();
        buffer.record(&"é".repeat(TURN_TEXT_HEAD_CHARS * 2));

        let text = buffer.take().expect("text was produced");
        assert!(text.head.chars().count() <= TURN_TEXT_HEAD_CHARS);
        assert!(
            text.head.ends_with('é'),
            "the head must not end mid-character"
        );
    }

    #[test]
    fn a_new_turn_does_not_inherit_the_previous_one() {
        let mut buffer = TurnTextBuffer::default();
        buffer.record("from the last turn");
        buffer.begin();
        assert!(buffer.take().is_none());
    }
}
