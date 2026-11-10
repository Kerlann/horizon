//! Save visibility while authoritative rows are lent out.
use super::*;

impl Game<'_> {
    /// FallingBlockEntity.tick: fall with 0.04 gravity and 0.98F drag, then land.
    /// One falling block's `tick`: places or drops its block in its own tick,
    /// so a block falling onto one that just landed lands on top of it. `i` is
    /// its place in the list; the caller has checked it is ticking.
    /// `entities` are the boxes that stop its move.
    pub(in crate::game) fn tick_one_falling(&mut self, i: usize, entities: &[Aabb]) -> Result<()> {
        let entity_drops = self
            .ui
            .rules
            .bool_at(horizon_client::gamerules::vanilla::ENTITY_DROPS);
        let _save_lease = horizon_client::join_seq::visibility::PassLease::enter();
        let mut f = self.falling.extract(self.falling[i].join).unwrap();
        let original = f.clone();
        match self.fall_step(&mut f, entity_drops, entities) {
            Ok(FallStep::Falling) => self.falling.put_back(i.min(self.falling.len()), f),
            Ok(FallStep::Gone) => {}
            Ok(FallStep::Dropped(at)) => self.drop_falling_block(f.state, at),
            Ok(FallStep::Landed(pos)) => self.finish_falling_block(f, pos),
            Err(error) => {
                horizon_client::join_seq::store_back(&mut f, original);
                self.falling.put_back(i.min(self.falling.len()), f);
                return Err(error);
            }
        }
        Ok(())
    }
}
