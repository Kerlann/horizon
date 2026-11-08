use super::{Put, Wire, WireError, Writer};
use std::cell::RefCell;
thread_local! { static BUFFERS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) }; }

// exhaustive: internal sink operations, consumed together by the transport
pub enum ArenaOp<'a> {
    Write(usize, &'a [u8]),
    Read(usize, &'a mut Writer),
}
// exhaustive: internal encoder state with private fields, constructed only by new
pub struct ArenaWriter<'a> {
    sink: &'a mut dyn FnMut(ArenaOp<'_>) -> Result<(), WireError>,
    len: usize,
    error: Option<WireError>,
    limit: Option<usize>,
}
impl<'a> ArenaWriter<'a> {
    pub fn new(sink: &'a mut dyn FnMut(ArenaOp<'_>) -> Result<(), WireError>) -> Self {
        Self {
            sink,
            len: 0,
            error: None,
            limit: None,
        }
    }
    pub fn put<T: Put + ?Sized>(&mut self, value: &T) {
        if self.error.is_none() {
            value.put_arena(self);
        }
    }
    pub fn put_owned<T: Wire>(&mut self, value: T) {
        if self.error.is_none() {
            value.put_owned_arena(self);
        }
    }
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        let Some(end) = self
            .len
            .checked_add(bytes.len())
            .filter(|n| *n <= self.limit.unwrap_or(u32::MAX as usize))
        else {
            self.fail(WireError::Length);
            return;
        };
        match (self.sink)(ArenaOp::Write(self.len, bytes)) {
            Ok(()) => self.len = end,
            Err(error) => self.fail(error),
        }
    }
    pub fn put_len(&mut self, len: usize) {
        match u32::try_from(len) {
            Ok(len) => self.put(&len),
            Err(_) => self.fail(WireError::Length),
        }
    }
    pub fn put_str(&mut self, text: &str) {
        self.put_len(text.len());
        self.put_bytes(text.as_bytes());
    }
    pub fn put_opt<T: Put + ?Sized>(&mut self, value: Option<&T>) {
        match value {
            Some(v) => {
                self.put(&1u8);
                self.put(v);
            }
            None => self.put(&0u8),
        }
    }
    pub fn fail(&mut self, error: WireError) {
        self.error.get_or_insert(error);
    }
    pub fn error(&self) -> Option<WireError> {
        self.error
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Preserve every custom `Writer` operation; only unknown codecs use this fallback.
    pub fn buffered(&mut self, encode: impl FnOnce(&mut Writer)) {
        if self.error.is_some() {
            return;
        }
        let mut writer = Writer::new();
        writer.replace(BUFFERS.with(|pool| pool.borrow_mut().pop().unwrap_or_default()));
        if let Err(error) = (self.sink)(ArenaOp::Read(self.len, &mut writer)) {
            self.fail(error);
        } else {
            if let Some(limit) = self.limit {
                writer.limit(limit);
            }
            encode(&mut writer);
            self.limit = writer.limit;
            match writer.error() {
                Some(error) => self.fail(error),
                None => {
                    // A custom codec may clear, replace, or transform the entire prefix.
                    self.len = 0;
                    self.put_bytes(writer.bytes());
                }
            }
        }
        let bytes = writer.into_bytes();
        if bytes.capacity() <= 1024 * 1024 {
            BUFFERS.with(|pool| {
                let mut pool = pool.borrow_mut();
                if pool.len() < 64 {
                    pool.push(bytes);
                }
            });
        }
    }
}
