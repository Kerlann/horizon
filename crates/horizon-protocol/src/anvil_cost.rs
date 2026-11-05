//! Full-width anvil cost synchronization between Horizon peers.
use crate::{
    Error, Result,
    buf::{Reader, Writer},
};

/// Clientbound custom payload channel.
pub const CHANNEL: &str = "horizon:anvil_cost";

/// Encode the window and its authoritative signed cost.
pub fn encode(window: i32, cost: i32) -> Vec<u8> {
    let mut w = Writer::new();
    w.varint(window);
    w.i32(cost);
    w.data
}

/// Decode an authoritative anvil cost, rejecting trailing data.
pub fn decode(data: &[u8]) -> Result<(i32, i32)> {
    let mut r = Reader::new(data);
    let window = r.varint()?;
    let cost = r.i32()?;
    if r.remaining() != 0 {
        return Err(Error::new("anvil cost: trailing bytes"));
    }
    Ok((window, cost))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_cost_survives_transport() {
        for cost in [i32::MIN, -65535, -7, 0, 1, i32::MAX] {
            assert_eq!(decode(&encode(42, cost)).unwrap(), (42, cost));
        }
        assert!(decode(&[42]).is_err());
        let mut payload = encode(42, -65535);
        payload.push(0);
        assert!(decode(&payload).is_err());
    }
}
