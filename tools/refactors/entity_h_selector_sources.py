#!/usr/bin/env python3
"""Split canonical selector read metadata from mutable goal objects for H loans.

Run from the repository root. The script is committed before its application;
source-carrier plumbing and behavioral corrections are separate commits.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
GOALS = ROOT / "crates/horizon-client/src/mobs/goals.rs"
SELECTOR = r'''#[derive(Clone, Debug)]
struct GoalRegistration {
    priority: i32,
    name: &'static str,
    running: std::cell::Cell<bool>,
}

/// A synchronous view of the selector's canonical registration state.
#[derive(Clone, Copy, Debug)]
pub struct GoalSelectorRead<'a> {
    entries: &'a [GoalRegistration],
}
impl<'a> GoalSelectorRead<'a> {
    pub fn entries(self) -> impl Iterator<Item = (i32, &'static str, bool)> + 'a {
        self.entries.iter().map(|entry| (entry.priority, entry.name, entry.running.get()))
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub enum SelectorKind { #[default] Goals, Targets }
#[derive(Clone, Copy, Debug, Default)]
pub struct GoalSelectorSources<'a> {
    pub goals: Option<GoalSelectorRead<'a>>,
    pub targets: Option<GoalSelectorRead<'a>>,
    pub active: SelectorKind,
}
impl<'a> GoalSelectorSources<'a> {
    fn with_active(mut self, source: GoalSelectorRead<'a>) -> Self {
        match self.active { SelectorKind::Goals => self.goals = Some(source), SelectorKind::Targets => self.targets = Some(source) }
        self
    }
}
enum GoalCallback { CanUse, CanContinue, Start, Stop, Tick }

#[derive(Clone, Debug, Default)]
pub struct GoalSelector {
    goals: Vec<Box<dyn Goal>>,
    registrations: Vec<GoalRegistration>,
    locked: [Option<usize>; 4],
    pub disabled: u8,
    loaned: bool,
}
impl GoalSelector {
    /// Move the actual selector out, leaving an explicitly unreadable loan slot.
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self { loaned: true, ..Self::default() })
    }
    pub fn read_source(&self) -> Option<GoalSelectorRead<'_>> {
        (!self.loaned).then_some(GoalSelectorRead { entries: &self.registrations })
    }
    pub fn add(&mut self, priority: i32, goal: impl Goal + 'static) {
        self.add_boxed(priority, Box::new(goal));
    }
    pub fn add_boxed(&mut self, priority: i32, goal: Box<dyn Goal>) {
        self.registrations.push(GoalRegistration {
            priority, name: goal.name(), running: std::cell::Cell::new(false),
        });
        self.goals.push(goal);
    }
    pub fn set_start_delay(&mut self, name: &str, ticks: i32) {
        for (goal, entry) in self.goals.iter_mut().zip(&self.registrations) {
            if entry.name == name { goal.set_start_delay(ticks); }
        }
    }
    pub fn clear(&mut self) {
        self.goals.clear();
        self.registrations.clear();
        self.locked = [None; 4];
    }
    pub fn running_names(&self) -> Vec<&'static str> {
        self.registrations.iter().filter(|entry| entry.running.get()).map(|entry| entry.name).collect()
    }
    pub fn has(&self, name: &str) -> bool {
        self.registrations.iter().any(|entry| entry.name == name)
    }
    pub fn is_running(&self, name: &str) -> bool {
        self.registrations.iter().any(|entry| entry.running.get() && entry.name == name)
    }
    fn invoke(&mut self, i: usize, mob: &mut Mob, cx: &mut Cx, callback: GoalCallback) -> bool {
        // The immutable registration slice and mutable goal objects are disjoint
        // canonical fields. Running changes use Cells, not a copied read shadow.
        let sources = cx.selectors.with_active(GoalSelectorRead { entries: &self.registrations });
        let mut scoped = Cx { w: cx.w, level: &mut *cx.level, events: &mut *cx.events, selectors: sources };
        let goal = &mut self.goals[i];
        match callback {
            GoalCallback::CanUse => goal.can_use(mob, &mut scoped),
            GoalCallback::CanContinue => goal.can_continue(mob, &mut scoped),
            GoalCallback::Start => { goal.start(mob, &mut scoped); false }
            GoalCallback::Stop => { goal.stop(mob, &mut scoped); false }
            GoalCallback::Tick => { goal.tick(mob, &mut scoped); false }
        }
    }
    fn stop(&mut self, i: usize, mob: &mut Mob, cx: &mut Cx) {
        if self.registrations[i].running.replace(false) { self.invoke(i, mob, cx, GoalCallback::Stop); }
    }
    pub fn tick(&mut self, mob: &mut Mob, cx: &mut Cx) {
        for i in 0..self.goals.len() {
            if self.registrations[i].running.get()
                && (self.goals[i].flags() & self.disabled != 0 || !self.invoke(i, mob, cx, GoalCallback::CanContinue)) {
                self.stop(i, mob, cx);
            }
        }
        for slot in &mut self.locked {
            if slot.is_some_and(|i| !self.registrations[i].running.get()) { *slot = None; }
        }
        for i in 0..self.goals.len() {
            let flags = self.goals[i].flags();
            if self.registrations[i].running.get() || flags & self.disabled != 0 { continue; }
            let replaceable = (0..4).filter(|bit| flags & (1 << bit) != 0).all(|bit| match self.locked[bit] {
                None => true,
                Some(j) => self.goals[j].interruptable() && self.registrations[i].priority < self.registrations[j].priority,
            });
            if !replaceable || !self.invoke(i, mob, cx, GoalCallback::CanUse) { continue; }
            for bit in 0..4 {
                if flags & (1 << bit) != 0 {
                    if let Some(j) = self.locked[bit] { self.stop(j, mob, cx); }
                    self.locked[bit] = Some(i);
                }
            }
            if !self.registrations[i].running.replace(true) { self.invoke(i, mob, cx, GoalCallback::Start); }
        }
        self.tick_running(mob, cx, true);
    }
    pub fn tick_running(&mut self, mob: &mut Mob, cx: &mut Cx, force: bool) {
        for i in 0..self.goals.len() {
            if self.registrations[i].running.get() && (force || self.goals[i].every_tick()) {
                self.invoke(i, mob, cx, GoalCallback::Tick);
            }
        }
    }
    pub fn stop_all(&mut self, mob: &mut Mob, cx: &mut Cx) {
        for i in 0..self.goals.len() { self.stop(i, mob, cx); }
        self.locked = [None; 4];
    }
    pub fn remove_all(&mut self, predicate: impl Fn(&dyn Goal) -> bool, mob: &mut Mob, cx: &mut Cx) -> usize {
        let mut removed = 0;
        let mut i = 0;
        while i < self.goals.len() {
            if predicate(&*self.goals[i]) {
                self.stop(i, mob, cx);
                self.goals.remove(i);
                self.registrations.remove(i);
                removed += 1;
                for slot in &mut self.locked {
                    match *slot { Some(j) if j == i => *slot = None, Some(j) if j > i => *slot = Some(j - 1), _ => {} }
                }
            } else { i += 1; }
        }
        removed
    }
    pub fn entries(&self) -> impl Iterator<Item = (i32, &'static str, bool)> + '_ {
        self.registrations.iter().map(|entry| (entry.priority, entry.name, entry.running.get()))
    }
}
'''


def split_fields(body):
    """The Cx literals only need their top-level comma-separated fields."""
    fields, start, depth = [], 0, 0
    for i, ch in enumerate(body):
        if ch in "([{": depth += 1
        elif ch in ")]}": depth -= 1
        elif ch == "," and depth == 0:
            fields.append(body[start:i]); start = i + 1
    fields.append(body[start:])
    return {m.group(1) for field in fields if (m := re.match(r"\s*(\w+)\s*(?::|$)", field))}


def apply():
    text = GOALS.read_text()
    start = text.index("#[derive(Clone, Debug)]\nstruct Wrapped {")
    end = text.index("// --- random positions", start)
    text = text[:start] + SELECTOR + "\n" + text[end:]
    text = text.replace("    pub events: &'a mut Vec<MobEvent>,", "    pub events: &'a mut Vec<MobEvent>,\n    pub selectors: GoalSelectorSources<'a>,", 1)
    text = text.replace("/// The goal's name for debugging (`Goal.toString`).", "/// The goal's registration name for debugging (`Goal.toString`).\n    /// A selector captures this identifier when the goal is added.", 1)
    GOALS.write_text(text)
    changed = 1
    for path in (ROOT / "crates").rglob("*.rs"):
        text = path.read_text()
        old = text
        text = re.sub(r"std::mem::take\(&mut ([\w.\[\]]+\.ai\.(?:goals|targets))\)", r"\1.take()", text)
        edits = []
        for match in re.finditer(r"\bCx\s*\{", text):
            start = match.end(); depth = 1; end = start
            while depth and end < len(text):
                if text[end] == "{": depth += 1
                elif text[end] == "}": depth -= 1
                end += 1
            body = text[start:end - 1]
            fields = split_fields(body)
            if {"w", "level", "events"} <= fields and "selectors" not in fields:
                insert = start + len(body.rstrip())
                comma = "" if body.rstrip().endswith(",") else ","
                edits.append((insert, comma + " selectors: Default::default(),"))
        for at, value in reversed(edits): text = text[:at] + value + text[at:]
        if text != old:
            path.write_text(text); changed += 1
    print(f"Changed {changed} source files; run rustfmt and then carrier corrections.")


if __name__ == "__main__":
    apply()
