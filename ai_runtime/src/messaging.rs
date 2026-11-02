//! Native agent messaging: structured, typed messages between agents via
//! real, working in-process mailboxes.
//!
//! ## Why function calls instead of `Researcher -> Reviewer` syntax
//!
//! The v1.0 brief illustrates messaging with `Researcher -> Reviewer`.
//! AILang already uses `->` for two other things — function return types
//! (`fn f() -> int`) and `workflow` stage chains (`A -> B -> C;` *inside* a
//! `workflow { }` block) — and neither of those is a general expression or
//! statement position. Adding a *third*, ambiguous meaning for a bare `A
//! -> B;` at the top level (send a message vs. a parse error vs. something
//! else entirely) risked real grammar ambiguity for a syntax-sugar-only
//! benefit. v1.0 delivers the identical capability — typed messages,
//! request/response, broadcast — through explicit stdlib functions
//! (`send_message`, `receive_message`, `broadcast`; see
//! `ailang-interpreter`'s builtins) instead, which is unambiguous and
//! exactly as expressive.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A single message in an agent's mailbox.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub from: String,
    pub text: String,
    pub timestamp: f64,
}

/// Every agent's mailbox, as real FIFO queues — genuinely working
/// synchronous message passing (no OS threads involved: this workspace's
/// `Value` type is `!Send`, see `ailang-interpreter`'s module doc on
/// `parallel {}` for why real concurrency needs a separate `Arc`/`Mutex`
/// migration), which is a faithful in-process model of the request/
/// response and broadcast patterns the v1.0 brief asks for, just not
/// literally running on separate threads or machines.
#[derive(Default)]
pub struct MailboxRegistry {
    boxes: RefCell<HashMap<String, VecDeque<Message>>>,
}

impl MailboxRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Delivers `text` from `from` into `to`'s mailbox (FIFO).
    pub fn send(&self, from: &str, to: &str, text: &str) {
        self.boxes
            .borrow_mut()
            .entry(to.to_string())
            .or_default()
            .push_back(Message {
                from: from.to_string(),
                text: text.to_string(),
                timestamp: now_seconds(),
            });
    }

    /// Delivers `text` from `from` to every name in `recipients` except
    /// `from` itself (a broadcast never messages its own sender).
    pub fn broadcast(&self, from: &str, recipients: &[String], text: &str) {
        for to in recipients {
            if to != from {
                self.send(from, to, text);
            }
        }
    }

    /// Pops and returns the oldest undelivered message for `agent`, or
    /// `None` if its mailbox is empty.
    pub fn receive(&self, agent: &str) -> Option<Message> {
        self.boxes.borrow_mut().get_mut(agent)?.pop_front()
    }

    /// Number of undelivered messages waiting for `agent`.
    pub fn pending(&self, agent: &str) -> usize {
        self.boxes.borrow().get(agent).map(|q| q.len()).unwrap_or(0)
    }
}

/// Publish/subscribe topics, layered on top of [`MailboxRegistry`]:
/// `publish` delivers to every subscriber's mailbox via the same FIFO
/// queues `send`/`receive` use, so `receive_message` works identically
/// whether a message arrived via direct send, broadcast, or a topic.
#[derive(Default)]
pub struct TopicRegistry {
    subscribers: RefCell<HashMap<String, Vec<String>>>,
}

impl TopicRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, topic: &str, agent: &str) {
        let mut subs = self.subscribers.borrow_mut();
        let list = subs.entry(topic.to_string()).or_default();
        if !list.iter().any(|a| a == agent) {
            list.push(agent.to_string());
        }
    }

    pub fn unsubscribe(&self, topic: &str, agent: &str) {
        if let Some(list) = self.subscribers.borrow_mut().get_mut(topic) {
            list.retain(|a| a != agent);
        }
    }

    pub fn subscribers(&self, topic: &str) -> Vec<String> {
        self.subscribers
            .borrow()
            .get(topic)
            .cloned()
            .unwrap_or_default()
    }

    /// Delivers `text` (tagged with sender `"topic:<topic>"`) to every
    /// current subscriber of `topic` via `mailboxes`.
    pub fn publish(&self, topic: &str, text: &str, mailboxes: &MailboxRegistry) {
        let sender = format!("topic:{topic}");
        for subscriber in self.subscribers(topic) {
            mailboxes.send(&sender, &subscriber, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_receive_a_single_message() {
        let mailboxes = MailboxRegistry::new();
        mailboxes.send("Researcher", "Reviewer", "draft ready");
        let msg = mailboxes.receive("Reviewer").unwrap();
        assert_eq!(msg.from, "Researcher");
        assert_eq!(msg.text, "draft ready");
    }

    #[test]
    fn messages_are_delivered_in_fifo_order() {
        let mailboxes = MailboxRegistry::new();
        mailboxes.send("A", "B", "first");
        mailboxes.send("A", "B", "second");
        assert_eq!(mailboxes.receive("B").unwrap().text, "first");
        assert_eq!(mailboxes.receive("B").unwrap().text, "second");
    }

    #[test]
    fn receive_returns_none_when_mailbox_is_empty() {
        let mailboxes = MailboxRegistry::new();
        assert!(mailboxes.receive("Nobody").is_none());
    }

    #[test]
    fn broadcast_reaches_every_recipient_except_the_sender() {
        let mailboxes = MailboxRegistry::new();
        let team = vec!["CEO".to_string(), "CTO".to_string(), "Designer".to_string()];
        mailboxes.broadcast("CEO", &team, "all hands meeting at 3pm");

        assert_eq!(mailboxes.pending("CEO"), 0); // sender doesn't message itself
        assert_eq!(mailboxes.pending("CTO"), 1);
        assert_eq!(mailboxes.pending("Designer"), 1);
        assert_eq!(mailboxes.receive("CTO").unwrap().from, "CEO");
    }

    #[test]
    fn pending_counts_undelivered_messages() {
        let mailboxes = MailboxRegistry::new();
        mailboxes.send("A", "B", "one");
        mailboxes.send("A", "B", "two");
        assert_eq!(mailboxes.pending("B"), 2);
        mailboxes.receive("B");
        assert_eq!(mailboxes.pending("B"), 1);
    }

    #[test]
    fn topic_publish_delivers_to_every_subscriber() {
        let topics = TopicRegistry::new();
        let mailboxes = MailboxRegistry::new();
        topics.subscribe("alerts", "A");
        topics.subscribe("alerts", "B");

        topics.publish("alerts", "server down", &mailboxes);

        assert_eq!(mailboxes.receive("A").unwrap().text, "server down");
        assert_eq!(mailboxes.receive("B").unwrap().text, "server down");
    }

    #[test]
    fn topic_publish_does_not_reach_unsubscribed_agents() {
        let topics = TopicRegistry::new();
        let mailboxes = MailboxRegistry::new();
        topics.subscribe("alerts", "A");
        topics.publish("alerts", "hello", &mailboxes);
        assert_eq!(mailboxes.pending("C"), 0);
    }

    #[test]
    fn unsubscribe_stops_further_delivery() {
        let topics = TopicRegistry::new();
        let mailboxes = MailboxRegistry::new();
        topics.subscribe("alerts", "A");
        topics.unsubscribe("alerts", "A");
        topics.publish("alerts", "hello", &mailboxes);
        assert_eq!(mailboxes.pending("A"), 0);
    }

    #[test]
    fn subscribing_twice_does_not_duplicate_delivery() {
        let topics = TopicRegistry::new();
        let mailboxes = MailboxRegistry::new();
        topics.subscribe("alerts", "A");
        topics.subscribe("alerts", "A");
        topics.publish("alerts", "hello", &mailboxes);
        assert_eq!(mailboxes.pending("A"), 1);
    }
}
