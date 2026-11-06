//! Mod goals on mobs (arismoko/horizon#1077): the parts a mod attaches to an
//! entity type register their goals at every `Mob` construction, right after
//! the species' own, and the engine wraps each in a [`ModGoal`] that gives the
//! mod its own mob, the tick's read view, its random and its writes.
//!
//! The read view answers through the generated `Backend` impl at the bottom:
//! the ticking mob itself, the player the tick sees and the tick's snapshot of
//! the other entities; every other backend read is refused its typed refusal,
//! as a host without a backend refuses it. The engine's own goals are
//! untouched, and a mob whose type no mod attached a part to pays nothing.
use super::{
    Mob, MobWorld,
    goals::{Cx, Goal as EngineGoal, GoalSelector},
};
use mod_api::{
    NativeWorldEpoch,
    crash::{NativeModCall, enter_native_mod},
};

/// The native side a mod goal runs against (the `mob_hooks` pattern): the
/// game's facts for the tick's read view, and the one call that runs a goal
/// with its live context, which only the game side can build. Handed to the
/// mob tick once beside `mob_world_base`, `None` in `MobWorld::goal_host`
/// wherever no mod attached an entity part — so no other side runs one.
pub trait GoalHost {
    /// The handles' epoch: a handle of an earlier world refuses.
    fn epoch(&self) -> Option<NativeWorldEpoch>;
    /// Run `goal`'s `op` on the mob in hand with the tick's `MobWorld`, naming
    /// `mod_id`'s `part` in a panic. None where this side runs no mod goals.
    fn run_goal(
        &self,
        goal: &mut Box<dyn mod_api::Goal>,
        m: &mut Mob,
        w: &MobWorld<'_>,
        op: GoalOp,
        mod_id: &'static str,
        part: &'static str,
    ) -> Option<bool>;
}

/// One mod goal call the engine's selectors make.
#[derive(Clone, Copy, Debug)]
pub enum GoalOp {
    /// `Goal::can_use`.
    CanUse,
    /// `Goal::can_continue`.
    CanContinue,
    /// `Goal::start`.
    Start,
    /// `Goal::stop`.
    Stop,
    /// `Goal::tick`.
    Tick,
}

/// Ask the installed entity parts for the mob's mod goals (#1077), right after
/// the species' own `ai::register` — vanilla's `registerGoals` moment, which a
/// mob loaded from a chunk passes through as a spawned one does.
pub fn attach(m: &mut Mob) {
    if !mod_api::__private::has_entity_parts() {
        return;
    }
    let parts = mod_api::__private::entity_parts();
    let list = parts.entity_parts(&m.kind.id);
    if list.is_empty() {
        return;
    }
    let (id, mob_id) = (m.id, &m.kind.id);
    for attached in list {
        let _guard = enter_native_mod(NativeModCall {
            id: attached.mod_id(),
            hook: "register_goals",
            part: Some(attached.type_name()),
        });
        let ai = &mut m.ai;
        let mut registrar = RegistrarFor {
            goals: &mut ai.goals,
            targets: &mut ai.targets,
            mob_id,
            mob_entity: id,
            mod_id: attached.mod_id(),
            part: attached.type_name(),
        };
        attached
            .part()
            .register_goals(&mut mod_api::GoalRegistrar::new(&mut registrar));
    }
}

/// The game's side of [`mod_api::GoalRegistrar`]: the mob's two selectors.
struct RegistrarFor<'a> {
    goals: &'a mut GoalSelector,
    targets: &'a mut GoalSelector,
    mob_id: &'a str,
    mob_entity: u32,
    mod_id: &'static str,
    part: &'static str,
}

impl mod_api::GoalRegistrarBackend for RegistrarFor<'_> {
    fn add_goal(&mut self, priority: i32, goal: Box<dyn mod_api::Goal>) {
        let goal = ModGoal::new(self.mob_entity, self.mod_id, self.part, goal);
        self.goals.add_boxed(priority, Box::new(goal));
    }

    fn add_target_goal(&mut self, priority: i32, goal: Box<dyn mod_api::Goal>) {
        let goal = ModGoal::new(self.mob_entity, self.mod_id, self.part, goal);
        self.targets.add_boxed(priority, Box::new(goal));
    }

    fn mob_id(&self) -> &str {
        self.mob_id
    }
}

/// One mod goal, wrapped for the engine's selectors: the mob it runs on, the
/// mod and part a panic in it names, and the goal itself. A clone of the mob
/// (a corpse kept for its death events) keeps the wrapper's shape with the
/// goal left out — a mod goal's state is its own and no copy of it exists, and
/// a corpse runs no goals.
pub struct ModGoal {
    mob: u32,
    mod_id: &'static str,
    part: &'static str,
    goal: Option<Box<dyn mod_api::Goal>>,
}

impl ModGoal {
    fn new(
        mob: u32,
        mod_id: &'static str,
        part: &'static str,
        goal: Box<dyn mod_api::Goal>,
    ) -> Self {
        Self {
            mob,
            mod_id,
            part,
            goal: Some(goal),
        }
    }

    /// Run `op` on the goal through the tick's goal host, which builds the
    /// context and names the mod in a panic. None — the caller's idle answer —
    /// where the goal is gone (a clone) or the side ticking is not the
    /// native-authoritative one (`goal_host`, as `mob_hooks` asks nothing
    /// there).
    fn run(&mut self, m: &mut Mob, cx: &mut Cx, op: GoalOp) -> Option<bool> {
        let goal = self.goal.as_mut()?;
        cx.w.goal_host?
            .run_goal(goal, m, cx.w, op, self.mod_id, self.part)
    }
}

impl std::fmt::Debug for ModGoal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModGoal")
            .field("mob", &self.mob)
            .field("goal", &self.goal.as_ref().map(|goal| goal.name()))
            .finish()
    }
}

impl EngineGoal for ModGoal {
    fn flags(&self) -> u8 {
        self.goal.as_ref().map_or(0, |goal| goal.flags().bits())
    }

    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.run(m, cx, GoalOp::CanUse).unwrap_or(false)
    }

    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.run(m, cx, GoalOp::CanContinue).unwrap_or(false)
    }

    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let _ = self.run(m, cx, GoalOp::Start);
    }

    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        let _ = self.run(m, cx, GoalOp::Stop);
    }

    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let _ = self.run(m, cx, GoalOp::Tick);
    }

    fn every_tick(&self) -> bool {
        self.goal
            .as_ref()
            .is_some_and(|goal| goal.requires_update_every_tick())
    }

    fn interruptable(&self) -> bool {
        self.goal
            .as_ref()
            .is_some_and(|goal| goal.is_interruptable())
    }

    fn box_clone(&self) -> Box<dyn EngineGoal> {
        Box::new(ModGoal {
            mob: self.mob,
            mod_id: self.mod_id,
            part: self.part,
            goal: None,
        })
    }

    fn name(&self) -> &'static str {
        self.goal.as_ref().map_or("ModGoal", |goal| goal.name())
    }
}
