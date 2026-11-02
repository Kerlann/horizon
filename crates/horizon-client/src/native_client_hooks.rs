//! Callbacks into the executable for native trait mods' client-library hooks.
use crate::items::ItemStack;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Client-library hooks, installed before any menu, entity or recipe is used.
pub struct NativeClientHooks {
    pub menu_slot_rules: fn(&str, usize, &ItemStack, &mut bool, &mut u32),
    pub living_breathe: fn(u32, &str, &mut bool, &mut i32, &mut i32),
    pub living_visibility: fn([i32; 4], Option<u32>, &mut f64),
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

pub(crate) fn living_visibility(
    player: [i32; 4],
    looking_entity: Option<u32>,
    visibility_modifier: &mut f64,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.living_visibility)(player, looking_entity, visibility_modifier);
    }
}

pub(crate) fn ingredient(kind: &str, data: &Map<String, Value>, items: &mut Vec<String>) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.ingredient)(kind, data, items);
    }
}
