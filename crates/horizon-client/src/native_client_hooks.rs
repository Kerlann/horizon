//! Callbacks into the executable for native trait mods' client-library hooks.
use crate::items::ItemStack;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Client-library hooks, installed before any menu, entity or recipe is used.
pub struct NativeClientHooks {
    pub menu_slot_rules: fn(&str, usize, &ItemStack, &mut bool, &mut u32),
    pub living_breathe: fn(u32, &str, &mut bool, &mut i32, &mut i32),
    pub mob_despawn: fn(u32, &str, &mut Option<bool>),
    pub spawn_cluster_size: fn(&str, [f64; 3], &mut i32),
    pub finalize_spawn: fn(&str, [f64; 3], &mut crate::mobs::spawning::FinalizeSpawn),
    pub mob_spawn_position_check: fn(&str, [f64; 3], &str, Option<[i32; 3]>, &mut Option<bool>),
    pub mob_spawn_placement_check: fn(&str, [i32; 3], &str, bool, &mut Option<bool>),
    pub ingredient: fn(&str, &Map<String, Value>, &mut Vec<String>),
}

static HOOKS: OnceLock<NativeClientHooks> = OnceLock::new();

/// Install the process-wide callbacks once; a second installation keeps the first.
pub fn install(hooks: NativeClientHooks) -> Result<(), NativeClientHooks> {
    HOOKS.set(hooks)
}

pub(crate) fn menu_slot_rules(
    menu: &str,
    slot: usize,
    stack: &ItemStack,
    may_place: &mut bool,
    max_stack: &mut u32,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.menu_slot_rules)(menu, slot, stack, may_place, max_stack);
    }
}

pub(crate) fn living_breathe(
    entity: u32,
    kind: &str,
    can_breathe: &mut bool,
    consume: &mut i32,
    refill: &mut i32,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_breathe)(entity, kind, can_breathe, consume, refill);
    }
}

pub(crate) fn mob_despawn(entity: u32, kind: &str, result: &mut Option<bool>) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mob_despawn)(entity, kind, result);
    }
}

pub(crate) fn finalize_spawn(
    kind: &str,
    position: [f64; 3],
    event: &mut crate::mobs::spawning::FinalizeSpawn,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.finalize_spawn)(kind, position, event);
    }
}

pub(crate) fn spawn_cluster_size(kind: &str, position: [f64; 3], size: &mut i32) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.spawn_cluster_size)(kind, position, size);
    }
}

pub(crate) fn mob_spawn_position_check(
    kind: &str,
    position: [f64; 3],
    reason: &str,
    spawner: Option<[i32; 3]>,
    result: &mut Option<bool>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mob_spawn_position_check)(kind, position, reason, spawner, result);
    }
}

pub(crate) fn mob_spawn_placement_check(
    kind: &str,
    position: [i32; 3],
    reason: &str,
    default_result: bool,
    result: &mut Option<bool>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mob_spawn_placement_check)(kind, position, reason, default_result, result);
    }
}

pub(crate) fn ingredient(kind: &str, data: &Map<String, Value>, items: &mut Vec<String>) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.ingredient)(kind, data, items);
    }
}
