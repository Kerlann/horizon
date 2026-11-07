//! The service fakes: the commands a mod runs and the messages it sends.
use crate::{Commands, Messages, Text};
use std::cell::RefCell;
use std::collections::HashSet;

/// The server's command service: the commands run, in order, of which those listed as known
/// succeed, and the feedback given.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct FakeCommands {
    known: HashSet<String>,
    /// Each command line run, in order.
    pub ran: Vec<String>,
    /// Each feedback line given, in order.
    pub feedback: Vec<Text>,
}

impl FakeCommands {
    /// Makes command line `line` succeed when run.
    pub fn known(&mut self, line: &str) -> &mut Self {
        self.known.insert(line.to_owned());
        self
    }
}

impl Commands for FakeCommands {
    fn run_command(&mut self, line: &str) -> bool {
        self.ran.push(line.to_owned());
        self.known.contains(line)
    }
    fn feedback(&mut self, line: Text) {
        self.feedback.push(line);
    }
}

/// The server's message service: the lines broadcast, in order, and the texts the chat filter
/// blocks.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct FakeMessages {
    blocked: HashSet<String>,
    broadcast: RefCell<Vec<Text>>,
}

impl FakeMessages {
    /// Makes the chat filter block text `text`.
    pub fn blocked(&mut self, text: &str) -> &mut Self {
        self.blocked.insert(text.to_owned());
        self
    }
    /// Each line broadcast, in order.
    pub fn broadcast_lines(&self) -> Vec<Text> {
        self.broadcast.borrow().clone()
    }
}

impl Messages for FakeMessages {
    fn broadcast(&self, line: Text) {
        self.broadcast.borrow_mut().push(line);
    }
    fn text_allowed(&self, text: &str) -> Option<bool> {
        Some(!self.blocked.contains(text))
    }
}
