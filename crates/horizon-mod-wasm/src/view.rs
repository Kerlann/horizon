//! The generic internals imports over one raw call's frame. Like the mod API's call frame,
//! pointers and nodes stay on the calling thread, never in the group's Send store data.
use crate::frame::{Call, Frame, Node, Reached};
use mod_api::__private::wasm_host::State;
use mod_points::internals::bindings::horizon::mod_internals::view as wire;
use mod_points::internals::{self, Table, flatten, unflatten};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use wasmtime::component::{HasData, Linker, Resource};

/// A declared-list entry resolved once at load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    Type(u32),
    Thunk(u32),
    Place(usize),
    Member { owner: u32, field: u32 },
}

/// Resolves only fingerprinted items; wrappers are reached from checked named types.
pub fn resolve(
    table: &Table,
    mod_id: &str,
    declared: &[(&str, &str)],
) -> Result<Vec<Resolved>, crate::Refusal> {
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(&(path, _)) = declared
        .iter()
        .find(|(path, _)| table.native_only.contains(path))
    {
        return Err(crate::Refusal::missing_internal(table, mod_id, path));
    }
    if table.types.is_empty()
        && table.thunks.is_empty()
        && table.places.is_empty()
        && table.members.is_empty()
        && table.native_only.is_empty()
    {
        return Err(crate::Refusal::NoInternalsTable);
    }
    declared
        .iter()
        .map(|&(path, hash)| {
            let candidates = table
                .types
                .iter()
                .enumerate()
                .filter(|(_, ty)| ty.path == path && !ty.hash.is_empty())
                .map(|(i, ty)| (Resolved::Type(i as u32), ty.hash))
                .chain(
                    table
                        .thunks
                        .iter()
                        .enumerate()
                        .filter(|(_, thunk)| thunk.path == path && !thunk.hash.is_empty())
                        .map(|(i, thunk)| (Resolved::Thunk(i as u32), thunk.hash)),
                )
                .chain(
                    table
                        .members
                        .iter()
                        .filter(|member| member.path == path && !member.hash.is_empty())
                        .map(|member| {
                            (
                                Resolved::Member {
                                    owner: member.owner,
                                    field: member.field,
                                },
                                member.hash,
                            )
                        }),
                )
                .chain(
                    table
                        .places
                        .iter()
                        .enumerate()
                        .filter(|(_, place)| place.target == path && !place.hash.is_empty())
                        .map(|(i, place)| (Resolved::Place(i), place.hash)),
                );
            let mut present = false;
            for (resolved, found) in candidates {
                present = true;
                if found != hash {
                    continue;
                }
                if let Resolved::Member { owner, .. } = resolved {
                    let owner = table
                        .types
                        .get(owner as usize)
                        .ok_or_else(|| crate::Refusal::ChangedInternal { path: path.into() })?;
                    if !declared.contains(&(owner.path, owner.hash)) {
                        return Err(crate::Refusal::ChangedInternal {
                            path: owner.path.into(),
                        });
                    }
                }
                return Ok(resolved);
            }
            Err(if present {
                crate::Refusal::ChangedInternal { path: path.into() }
            } else {
                crate::Refusal::missing_internal(table, mod_id, path)
            })
        })
        .collect()
}

struct KeptNode {
    node: Node,
    bytes: usize,
}

struct Active<'a> {
    frame: Frame<'a>,
    nodes: BTreeMap<u32, KeptNode>,
    node_path_bytes: usize,
    declared: &'a [Resolved],
}

thread_local! {
    static CURRENT: Cell<*mut Active<'static>> = const { Cell::new(std::ptr::null_mut()) };
}
// Never reuse a resource rep, even if a group's next call runs on a different thread.
static NEXT_NODE: AtomicU32 = AtomicU32::new(1);

fn gone(why: &str) -> wire::Refusal {
    wire::Refusal::Gone(why.into())
}

fn current<T>(
    f: impl FnOnce(&mut Active<'_>) -> Result<T, wire::Refusal>,
) -> Result<T, wire::Refusal> {
    // SAFETY: `run` installs a stack frame for the synchronous call, restoring it before it
    // drops. Imports run serially on that thread; no reference escapes this closure.
    let active = unsafe { CURRENT.get().as_mut() }.ok_or_else(|| gone("no call is running"))?;
    f(active)
}

impl Active<'_> {
    fn node(&self, resource: &Resource<wire::Node>) -> Result<&Node, wire::Refusal> {
        self.nodes
            .get(&resource.rep())
            .map(|held| &held.node)
            .ok_or_else(|| gone("the node's call is over"))
    }

    fn keep(&mut self, node: Node) -> Result<Resource<wire::Node>, wire::Refusal> {
        let bytes = crate::frame::path_bytes(&node.path)?;
        if self.nodes.len() >= 4096 || self.node_path_bytes + bytes > 1_048_576 {
            return Err(wire::Refusal::NotHere("internals budget".into()));
        }
        let rep = NEXT_NODE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| gone("node resource ids exhausted"))?;
        self.nodes.insert(rep, KeptNode { node, bytes });
        self.node_path_bytes += bytes;
        Ok(Resource::new_own(rep))
    }

    fn reached(&mut self, reached: Reached) -> Result<wire::Reached, wire::Refusal> {
        match reached {
            Reached::Node(node) => self.keep(node).map(wire::Reached::Node),
            Reached::Value(value) => encode(value).map(wire::Reached::Value),
        }
    }
}

struct Scope(*mut Active<'static>);
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.set(self.0);
    }
}

/// Runs a raw listener with a call-scoped view; commits only when the listener returns normally.
/// A trap drops its pending writes, and the scope guard restores the previous thread frame.
pub fn run<T>(
    mut frame: Frame<'_>,
    declared: &[Resolved],
    f: impl FnOnce() -> wasmtime::Result<T>,
) -> wasmtime::Result<T> {
    frame.allow_members(declared.iter().filter_map(|entry| match entry {
        Resolved::Member { owner, field } => Some((*owner, *field)),
        _ => None,
    }));
    let mut active = Active {
        frame,
        nodes: BTreeMap::new(),
        node_path_bytes: 0,
        declared,
    };
    let scope = Scope(CURRENT.replace((&raw mut active).cast()));
    let result = contain(f);
    drop(scope);
    if result.is_ok() {
        contain(|| {
            log_refused(active.frame.apply());
            Ok(())
        })?;
    }
    result
}

fn log_refused(
    refused: Vec<(
        crate::journal::Path<internals::Value>,
        u32,
        internals::Refusal,
    )>,
) {
    for (path, field, why) in refused {
        log::warn!("wasm internals: dropped write at {path:?} field {field}: {why:?}");
    }
}

/// A stateless host adapter: all imports use the innermost call on the current thread.
pub struct View;
impl HasData for View {
    type Data<'a> = View;
}

/// Adds the internals imports to a group's linker.
pub fn add_to_linker(linker: &mut Linker<State>) {
    wire::add_to_linker::<State, View>(linker, |_| View)
        .expect("the game's internals imports are defined once");
}

fn contain<T>(f: impl FnOnce() -> wasmtime::Result<T>) -> wasmtime::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        log::error!("wasm internals panic: {message}");
        Err(wasmtime::Error::msg(format!(
            "wasm internals panic: {message}"
        )))
    })
}

fn charge(mut host: impl wasmtime::AsContextMut, cost: usize) -> wasmtime::Result<()> {
    let mut store = host.as_context_mut();
    if store.engine().get_consume_fuel() {
        let fuel = store.get_fuel()?;
        let left = fuel
            .checked_sub(cost as u64)
            .ok_or_else(|| wasmtime::Error::msg("internals fuel exhausted"))?;
        store.set_fuel(left)?;
    }
    Ok(())
}

fn import<T>(
    host: wasmtime::component::Access<'_, State, View>,
    cells: usize,
    f: impl FnOnce() -> Result<T, wire::Refusal>,
    output: impl FnOnce(&T) -> usize,
) -> wasmtime::Result<Result<T, wire::Refusal>> {
    metered(host, cells, f, |result| result.as_ref().map_or(0, output))
}

fn metered<T>(
    mut host: wasmtime::component::Access<'_, State, View>,
    cells: usize,
    f: impl FnOnce() -> T,
    output: impl FnOnce(&T) -> usize,
) -> wasmtime::Result<T> {
    contain(|| {
        #[cfg(test)]
        let fuel_before = wasmtime::AsContextMut::as_context_mut(&mut host)
            .get_fuel()
            .ok();
        charge(&mut host, 1 + cells)?;
        let result = f();
        charge(&mut host, output(&result))?;
        #[cfg(test)]
        if let Some(fuel) = fuel_before {
            let left = wasmtime::AsContextMut::as_context_mut(&mut host).get_fuel()?;
            tests::FUEL_DEBITS.with_borrow_mut(|debits| debits.push(fuel - left));
        }
        Ok(result)
    })
}

fn wire_cost(cells: &[wire::Cell]) -> usize {
    let bytes: usize = cells
        .iter()
        .map(|cell| match cell {
            wire::Cell::Str(s) => s.len(),
            _ => 0,
        })
        .sum();
    cells.len().saturating_add(bytes.div_ceil(64))
}

fn node_cost(node: &Resource<wire::Node>) -> usize {
    current(|active| Ok(crate::frame::walk_cost(&active.node(node)?.path))).unwrap_or(0)
}

fn journal_cost() -> usize {
    current(|active| Ok(active.frame.journal_len())).unwrap_or(0)
}

fn reached_cells(value: &wire::Reached) -> usize {
    match value {
        wire::Reached::Value(v) => wire_cost(v),
        _ => 0,
    }
}

impl wire::HostNode for View {}
impl wire::Host for View {}
impl wire::HostNodeWithStore<State> for View {
    fn field(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
        n: u32,
    ) -> wasmtime::Result<Result<Resource<wire::Node>, wire::Refusal>> {
        import(host, node_cost(&node), || View.field(node, n), |_| 0)
    }
    fn get(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
        n: u32,
    ) -> wasmtime::Result<Result<Vec<wire::Cell>, wire::Refusal>> {
        import(
            host,
            node_cost(&node).saturating_add(journal_cost()),
            || View.get(node, n),
            |v| wire_cost(v),
        )
    }
    fn set(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
        n: u32,
        v: Vec<wire::Cell>,
    ) -> wasmtime::Result<Result<(), wire::Refusal>> {
        import(
            host,
            node_cost(&node).saturating_add(wire_cost(&v)),
            || View.set(node, n, v),
            |_| 0,
        )
    }
    fn index(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
        i: u64,
    ) -> wasmtime::Result<Result<wire::Reached, wire::Refusal>> {
        import(
            host,
            node_cost(&node),
            || View.index(node, i),
            reached_cells,
        )
    }
    fn key(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
        k: Vec<wire::Cell>,
    ) -> wasmtime::Result<Result<wire::Reached, wire::Refusal>> {
        import(
            host,
            node_cost(&node).saturating_add(wire_cost(&k)),
            || View.key(node, k),
            reached_cells,
        )
    }
    fn len(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
    ) -> wasmtime::Result<Result<u64, wire::Refusal>> {
        import(host, node_cost(&node), || View.len(node), |_| 0)
    }
    fn case(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
    ) -> wasmtime::Result<Result<u32, wire::Refusal>> {
        import(host, node_cost(&node), || View.case(node), |_| 0)
    }
    fn read(
        host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
    ) -> wasmtime::Result<Result<Vec<wire::Cell>, wire::Refusal>> {
        import(
            host,
            node_cost(&node).saturating_add(journal_cost()),
            || View.read(node),
            |v| wire_cost(v),
        )
    }
    fn drop(
        mut host: wasmtime::component::Access<'_, State, Self>,
        node: Resource<wire::Node>,
    ) -> wasmtime::Result<()> {
        contain(|| {
            charge(&mut host, 1)?;
            View.drop(node)
        })
    }
}
impl wire::HostWithStore<State> for View {
    fn get_many(
        host: wasmtime::component::Access<'_, State, Self>,
        reads: Vec<(Resource<wire::Node>, u32)>,
    ) -> wasmtime::Result<Vec<Result<Vec<wire::Cell>, wire::Refusal>>> {
        let reads = bounded_batch(reads)?;
        let cells = reads
            .iter()
            .map(|(n, _)| node_cost(n).saturating_add(journal_cost()))
            .fold(reads.len().saturating_sub(1), usize::saturating_add);
        metered(
            host,
            cells,
            || View.get_many(reads),
            |results| {
                results
                    .iter()
                    .filter_map(|r| r.as_ref().ok())
                    .map(|v| wire_cost(v))
                    .fold(0, usize::saturating_add)
            },
        )
    }
    fn set_many(
        host: wasmtime::component::Access<'_, State, Self>,
        writes: Vec<(Resource<wire::Node>, u32, Vec<wire::Cell>)>,
    ) -> wasmtime::Result<Vec<Result<(), wire::Refusal>>> {
        let writes = bounded_batch(writes)?;
        let cells = writes
            .iter()
            .map(|(n, _, v)| node_cost(n).saturating_add(wire_cost(v)))
            .fold(writes.len().saturating_sub(1), usize::saturating_add);
        metered(host, cells, || View.set_many(writes), |_| 0)
    }
    fn root(
        host: wasmtime::component::Access<'_, State, Self>,
        n: u32,
    ) -> wasmtime::Result<Result<Resource<wire::Node>, wire::Refusal>> {
        import(host, 0, || View.root(n), |_| 0)
    }
    fn call(
        host: wasmtime::component::Access<'_, State, Self>,
        declared: u32,
        receiver: Option<Resource<wire::Node>>,
        args: Vec<wire::Arg>,
    ) -> wasmtime::Result<Result<wire::CallReply, wire::Refusal>> {
        let cells = args
            .iter()
            .map(|a| match a {
                wire::Arg::Value(v) => wire_cost(v),
                wire::Arg::Node(n) => node_cost(n),
            })
            .fold(
                receiver.as_ref().map_or(0, node_cost),
                usize::saturating_add,
            );
        import(
            host,
            1000usize
                .saturating_add(cells)
                .saturating_add(journal_cost()),
            || View.call(declared, receiver, args),
            |reply| {
                reply
                    .updates
                    .iter()
                    .map(|cells| wire_cost(cells))
                    .fold(reached_cells(&reply.result), usize::saturating_add)
            },
        )
    }
}

fn bounded_batch<T>(entries: Vec<T>) -> wasmtime::Result<Vec<T>> {
    wasmtime::ensure!(entries.len() <= 12, "internals batch entry budget");
    Ok(entries)
}

fn encode(value: internals::Value) -> Result<Vec<wire::Cell>, wire::Refusal> {
    crate::frame::value_size(&value)?;
    Ok(flatten(value))
}

fn decode(cells: Vec<wire::Cell>) -> Result<internals::Value, wire::Refusal> {
    if cells.len() > 65_536
        || cells
            .iter()
            .map(|c| match c {
                wire::Cell::Str(s) => s.len(),
                _ => 0,
            })
            .sum::<usize>()
            > 1_048_576
    {
        return Err(wire::Refusal::NotHere("internals budget".into()));
    }
    unflatten(cells).map_err(Into::into)
}

impl View {
    pub fn get_many(
        &mut self,
        reads: Vec<(Resource<wire::Node>, u32)>,
    ) -> Vec<Result<Vec<wire::Cell>, wire::Refusal>> {
        reads
            .into_iter()
            .map(|(node, field)| self.get(node, field))
            .collect()
    }
    pub fn set_many(
        &mut self,
        writes: Vec<(Resource<wire::Node>, u32, Vec<wire::Cell>)>,
    ) -> Vec<Result<(), wire::Refusal>> {
        writes
            .into_iter()
            .map(|(node, field, value)| self.set(node, field, value))
            .collect()
    }
    pub fn field(
        &mut self,
        node: Resource<wire::Node>,
        n: u32,
    ) -> Result<Resource<wire::Node>, wire::Refusal> {
        current(|a| {
            let next = a.frame.field(a.node(&node)?, n)?;
            a.keep(next)
        })
    }
    pub fn get(
        &mut self,
        node: Resource<wire::Node>,
        n: u32,
    ) -> Result<Vec<wire::Cell>, wire::Refusal> {
        current(|a| encode(a.frame.get(a.node(&node)?, n)?))
    }
    pub fn set(
        &mut self,
        node: Resource<wire::Node>,
        n: u32,
        v: Vec<wire::Cell>,
    ) -> Result<(), wire::Refusal> {
        current(|a| {
            let node = a
                .nodes
                .get(&node.rep())
                .ok_or_else(|| gone("the node's call is over"))?;
            a.frame.set(&node.node, n, decode(v)?).map_err(Into::into)
        })
    }
    pub fn index(
        &mut self,
        node: Resource<wire::Node>,
        i: u64,
    ) -> Result<wire::Reached, wire::Refusal> {
        current(|a| {
            let next = a.frame.index(a.node(&node)?, i)?;
            a.reached(next)
        })
    }
    pub fn key(
        &mut self,
        node: Resource<wire::Node>,
        k: Vec<wire::Cell>,
    ) -> Result<wire::Reached, wire::Refusal> {
        current(|a| {
            let next = a.frame.key(a.node(&node)?, decode(k)?)?;
            a.reached(next)
        })
    }
    pub fn len(&mut self, node: Resource<wire::Node>) -> Result<u64, wire::Refusal> {
        current(|a| a.frame.len(a.node(&node)?).map_err(Into::into))
    }
    pub fn case(&mut self, node: Resource<wire::Node>) -> Result<u32, wire::Refusal> {
        current(|a| a.frame.case(a.node(&node)?).map_err(Into::into))
    }
    pub fn read(&mut self, node: Resource<wire::Node>) -> Result<Vec<wire::Cell>, wire::Refusal> {
        current(|a| encode(a.frame.read(a.node(&node)?)?))
    }
    pub fn drop(&mut self, node: Resource<wire::Node>) -> wasmtime::Result<()> {
        let _ = current(|a| {
            if let Some(held) = a.nodes.remove(&node.rep()) {
                a.node_path_bytes -= held.bytes;
            }
            Ok(())
        });
        Ok(())
    }
}

impl View {
    pub fn root(&mut self, n: u32) -> Result<Resource<wire::Node>, wire::Refusal> {
        current(|a| {
            let node = a.frame.root(n)?;
            a.keep(node)
        })
    }
    pub fn call(
        &mut self,
        declared: u32,
        receiver: Option<Resource<wire::Node>>,
        args: Vec<wire::Arg>,
    ) -> Result<wire::CallReply, wire::Refusal> {
        current(|a| {
            let Some(Resolved::Thunk(function)) = a.declared.get(declared as usize) else {
                return Err(wire::Refusal::NotHere("not a declared function".into()));
            };
            if let Some(receiver) = &receiver {
                a.nodes
                    .get_mut(&receiver.rep())
                    .ok_or_else(|| gone("the node's call is over"))?
                    .node
                    .restricted = true;
            }
            let nodes = &a.nodes;
            let node = |r: &Resource<wire::Node>| {
                nodes
                    .get(&r.rep())
                    .map(|held| &held.node)
                    .ok_or_else(|| gone("the node's call is over"))
            };
            let receiver = receiver.as_ref().map(node).transpose()?;
            let args = args
                .into_iter()
                .map(|arg| match arg {
                    wire::Arg::Value(v) => Ok(Call::Value(decode(v)?)),
                    wire::Arg::Node(r) => node(&r).map(Call::Node),
                })
                .collect::<Result<Vec<_>, wire::Refusal>>()?;
            // A game function may dispatch into another wasm group. Its imports must not
            // see this listener's frame, nor alias the mutable borrow held by this call.
            let mask = Scope(CURRENT.replace(std::ptr::null_mut()));
            let (reply, refused) = a.frame.call(*function, receiver, args)?;
            drop(mask);
            log_refused(refused);
            let updates = reply
                .updates
                .into_iter()
                .map(encode)
                .collect::<Result<_, _>>()?;
            Ok(wire::CallReply {
                result: a.reached(reply.result)?,
                updates,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wire_cost_charges_string_bytes() {
        let small = super::wire_cost(&[super::wire::Cell::Str("x".into())]);
        let large = super::wire_cost(&[super::wire::Cell::Str("x".repeat(1_048_576))]);
        assert!(large > small * 1000);
    }

    use super::*;
    use crate::frame::Root;
    use crate::frame::tests::{TABLE, frame, world};

    thread_local! {
        // Observe actual store debits, excluding fuel spent executing guest instructions.
        pub(super) static FUEL_DEBITS: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn take_debits() -> Vec<u64> {
        FUEL_DEBITS.with_borrow_mut(std::mem::take)
    }

    #[test]
    fn journal_cost_counts_pending_entries_and_resets_after_a_call_flushes_them() {
        let mut game = world();
        assert_eq!(journal_cost(), 0);
        run(
            frame(&mut game),
            &[Resolved::Thunk(0), Resolved::Member { owner: 0, field: 1 }],
            || {
                let root = View.root(0).unwrap();
                assert_eq!(journal_cost(), 0);
                for _ in 0..4096 {
                    View.set(borrow(&root), 1, vec![wire::Cell::B(true)])
                        .unwrap();
                }
                assert_eq!(journal_cost(), 4096);
                View.call(0, Some(borrow(&root)), vec![]).unwrap();
                assert_eq!(journal_cost(), 0);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(journal_cost(), 0);
    }

    #[test]
    fn live_nodes_and_wire_values_are_bounded() {
        let mut game = world();
        run(frame(&mut game), &[], || {
            for _ in 0..4096 { View.root(0).unwrap(); }
            assert!(matches!(View.root(0), Err(wire::Refusal::NotHere(why)) if why == "internals budget"));
            Ok(())
        }).unwrap();
        assert!(matches!(
            decode(vec![wire::Cell::Str("x".repeat(1_048_577))]),
            Err(wire::Refusal::NotHere(_))
        ));
        assert!(matches!(
            decode(vec![wire::Cell::None; 65_537]),
            Err(wire::Refusal::NotHere(_))
        ));
    }

    #[test]
    fn live_node_paths_hit_the_byte_budget_before_the_node_count() {
        use mod_points::internals::{Op, Reply, Type};
        unsafe fn cycle(ptr: *mut (), w: bool, op: Op<'_>) -> Result<Reply, internals::Refusal> {
            match op {
                Op::Field(0) => Ok(Reply::Node(ptr, 0, w)),
                _ => Err(internals::not_here("cycle")),
            }
        }
        let table = Table {
            types: &[Type {
                path: "Cycle",
                hash: "",
                node: cycle,
                raw_fields: &[0],
                raw_index: false,
            }],
            ..Table::EMPTY
        };
        let mut value = ();
        let frame = unsafe {
            Frame::new(
                &table,
                vec![Root {
                    ptr: (&raw mut value).cast(),
                    ty: 0,
                    writable: true,
                    receiver: false,
                    born: None,
                }],
            )
        };
        run(frame, &[], || {
            let mut next = View.root(0).unwrap();
            for _ in 0..4095 {
                match View.field(borrow(&next), 0) {
                    Ok(node) => next = node,
                    Err(wire::Refusal::NotHere(why)) if why == "internals budget" => return Ok(()),
                    other => panic!("unexpected path result: {other:?}"),
                }
            }
            panic!("path bytes must exhaust before the node count");
        })
        .unwrap();
    }

    mod fixture {
        include!("../tests/internals/host.rs");
    }

    type FixtureFire = wasmtime::component::TypedFunc<(u32, mod_points::internals::bindings::exports::horizon::mod_internals::raw_listeners::HostKind), ()>;
    struct FixtureGroup {
        exports: mod_api::__private::wasm_host::Exports,
        fire: FixtureFire,
        declared: Vec<Resolved>,
    }
    fn fixture_group() -> FixtureGroup {
        fixture_group_with_fuel(false)
    }
    fn fixture_group_with_fuel(fuel: bool) -> FixtureGroup {
        use mod_api::__private::wasm_host::{Member, instantiate};
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_WASM_INTERNALS").expect("fixture directory"),
        );
        let package = root.join("counter/target/horizon-mod/internals_counter.hmod");
        let manifest = horizon_mods::Package::read(&package).unwrap().manifest;
        let package = crate::WasmPackage {
            manifest,
            module: horizon_mods::Package::read_wasm(&package).unwrap(),
        };
        let std = std::fs::read(std::env::var_os("HORIZON_WASM_STD").expect("std member")).unwrap();
        let mut config = crate::RuntimeConfig::default();
        // These tests must exercise the import refusal, not race the normal hook deadline.
        config.budgets.call = std::time::Duration::from_secs(30);
        if fuel {
            config.budgets.fuel = Some(Default::default());
        }
        let engine = crate::engine(&config).unwrap();
        let component =
            crate::component(&engine, &config, &std, std::slice::from_ref(&package), &[0]).unwrap();
        let exports = instantiate(
            &engine,
            &component,
            &[Member {
                id: "internals_counter",
                exports: horizon_mods::manifest::export_prefix("internals_counter"),
            }],
            config.budgets,
            |linker| {
                add_to_linker(linker);
            },
        )
        .unwrap()
        .remove(0)
        .exports;
        let interface = format!(
            "{}-raw-listeners",
            horizon_mods::manifest::export_prefix("internals_counter")
        );
        let listed = exports
            .func::<(), (Vec<(String, String)>,)>(&interface, "declared")
            .unwrap();
        let listed = exports.load(|store| Ok(listed.call(store, ())?.0)).unwrap();
        let declared = resolve(
            fixture::table(),
            "internals_counter",
            &listed
                .iter()
                .map(|(p, h)| (p.as_str(), h.as_str()))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let fire = exports.func(&interface, "fire").unwrap();
        FixtureGroup {
            exports,
            fire,
            declared,
        }
    }

    fn fixture_call(group: &FixtureGroup, game: &mut fixture::Game, message: &str) -> Option<()> {
        fixture_result(group, game, message).ok()
    }

    fn fixture_result(
        group: &FixtureGroup,
        game: &mut fixture::Game,
        message: &str,
    ) -> Result<(), String> {
        use mod_api::__private::wasm_host::CallHost;
        use mod_points::internals::bindings::exports::horizon::mod_internals::raw_listeners::HostKind;
        let mut record = fixture::Record {
            message: message.into(),
            cancel: false,
        };
        let table = fixture::table();
        let frame = unsafe {
            Frame::new(
                table,
                vec![
                    Root {
                        ptr: (game as *mut fixture::Game).cast(),
                        ty: 0,
                        writable: true,
                        receiver: true,
                        born: None,
                    },
                    Root {
                        ptr: (&raw mut record).cast(),
                        ty: 1,
                        writable: true,
                        receiver: false,
                        born: None,
                    },
                ],
            )
        };
        let mut error = None;
        let result = group.exports.call(CallHost::None, |store| {
            run(frame, &group.declared, || {
                group.fire.call(store, (0, HostKind::None))
            })
            .inspect_err(|why| error = Some(format!("{why:#}")))
        });
        result.ok_or_else(|| error.unwrap_or_else(|| "call refused".into()))
    }

    fn fixture_game() -> fixture::Game {
        fixture::Game {
            left: 1,
            right: 10,
            hidden: vec![],
            refused_sets: 0,
            reads: 0,
        }
    }

    fn assert_healthy_group_runs(healthy: &FixtureGroup) {
        let mut other = fixture_game();
        assert_eq!(fixture_call(healthy, &mut other, "before"), Some(()));
        assert!(!healthy.exports.stopped());
        assert_eq!((other.left, other.right), (8, 13));
    }

    fn refuses_group_but_not_another(mode: &str, expected_left: i64) {
        let broken = fixture_group();
        let healthy = fixture_group();
        let mut game = fixture_game();
        assert!(fixture_call(&broken, &mut game, mode).is_none());
        assert!(broken.exports.stopped());
        assert_eq!((game.left, game.right), (expected_left, 10));
        assert!(fixture_call(&broken, &mut game, "before").is_none());
        let mut other = fixture_game();
        assert_eq!(fixture_call(&healthy, &mut other, "before"), Some(()));
        assert!(!healthy.exports.stopped());
        assert_eq!((other.left, other.right), (8, 13));
    }

    #[test]
    #[ignore = "needs compiled internals fixture and std member"]
    fn thunk_panic_stops_only_its_group_after_flushing_prior_writes() {
        refuses_group_but_not_another("panic_thunk", 42);
    }

    #[test]
    #[ignore = "needs compiled internals fixture and std member"]
    fn final_apply_panic_stops_only_its_group_and_never_unwinds_into_game() {
        refuses_group_but_not_another("panic_apply", 1);
    }

    #[test]
    #[ignore = "needs compiled internals fixture and std member"]
    fn guest_journal_overflow_refuses_without_affecting_another_group() {
        refuses_group_but_not_another("budget", 1);
    }

    #[test]
    #[ignore = "needs compiled internals fixture and std member"]
    fn reused_large_string_exhausts_fuel_without_stopping_another_group() {
        let large = fixture_group_with_fuel(true);
        let small = fixture_group_with_fuel(true);
        let healthy = fixture_group_with_fuel(true);
        let mut large_game = fixture_game();
        let mut small_game = fixture_game();
        take_debits();
        let error = fixture_result(&large, &mut large_game, &"x".repeat(1_048_576)).unwrap_err();
        assert!(error.contains("internals fuel exhausted"), "{error}");
        let large_debits = take_debits();
        assert_eq!(fixture_result(&small, &mut small_game, "x"), Ok(()));
        let small_debits = take_debits();
        let large_debit = *large_debits.iter().max().unwrap();
        let small_debit = *small_debits.iter().max().unwrap();
        assert!(
            large_debit >= 1000 * small_debit,
            "large={large_debit}, small={small_debit}"
        );
        // Each charged setter reaches the wrong-type check, never a retained write.
        // The initial message get has the same debit as one refused setter.
        assert!(large_game.refused_sets > 1);
        assert_eq!(
            large_debits.iter().filter(|&&n| n == large_debit).count(),
            large_game.refused_sets + 1
        );
        assert_eq!(small_game.refused_sets, 2000);
        assert_eq!(
            small_debits.iter().filter(|&&n| n == small_debit).count(),
            small_game.refused_sets + 1
        );
        assert_eq!((large_game.left, large_game.right), (1, 10));
        assert!(large.exports.stopped());
        assert_healthy_group_runs(&healthy);
    }

    #[test]
    #[ignore = "needs compiled internals fixture and std member"]
    fn full_journal_get_charges_scans_and_exhausts_only_its_groups_fuel() {
        let full = fixture_group_with_fuel(true);
        let empty = fixture_group_with_fuel(true);
        let healthy = fixture_group_with_fuel(true);
        let mut full_game = fixture_game();
        let mut empty_game = fixture_game();
        take_debits();
        let error = fixture_result(&full, &mut full_game, "full_journal").unwrap_err();
        assert!(error.contains("internals fuel exhausted"), "{error}");
        let full_debits = take_debits();
        assert_eq!(
            fixture_result(&empty, &mut empty_game, "empty_journal"),
            Ok(())
        );
        let empty_debits = take_debits();
        // Identify the measured get debits by their count, not guest instruction fuel.
        let full_get = full_debits.iter().copied().max().unwrap();
        let empty_get = *empty_debits.iter().find(|&&n| n == 2).unwrap();
        assert_eq!(
            empty_debits.iter().filter(|&&n| n == empty_get).count(),
            2000
        );
        assert!(
            full_get >= 1000 * empty_get,
            "full={full_get}, empty={empty_get}"
        );
        assert!(full_game.reads > 1);
        assert_eq!(
            full_debits.iter().filter(|&&n| n == full_get).count(),
            full_game.reads
        );
        assert_eq!(empty_game.reads, 2000);
        assert_eq!(
            (full_game.left, full_game.right),
            (1, 10),
            "trapping discards the filled journal"
        );
        assert!(full.exports.stopped());
        assert_healthy_group_runs(&healthy);
    }

    fn borrow(node: &Resource<wire::Node>) -> Resource<wire::Node> {
        Resource::new_borrow(node.rep())
    }

    #[test]
    fn batches_preserve_entry_results_and_journal_order() {
        let mut game = world();
        let declared = [Resolved::Member { owner: 0, field: 1 }];
        run(frame(&mut game), &declared, || {
            let root = View.root(0).unwrap();
            let results = View.set_many(vec![
                (borrow(&root), 1, vec![wire::Cell::B(false)]),
                (borrow(&root), 99, vec![wire::Cell::B(true)]),
                (borrow(&root), 1, vec![wire::Cell::B(true)]),
            ]);
            assert!(results[0].is_ok() && results[1].is_err() && results[2].is_ok());
            let results = View.get_many(vec![(borrow(&root), 99), (borrow(&root), 1)]);
            assert!(results[0].is_err());
            let single = unflatten(View.get(borrow(&root), 1).unwrap()).unwrap();
            assert_eq!(single, internals::Value::B(true));
            assert_eq!(
                unflatten(results.into_iter().nth(1).unwrap().unwrap()).unwrap(),
                single
            );
            assert!(View.get_many(vec![]).is_empty());
            assert!(View.set_many(vec![]).is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    #[ignore = "needs the internals guest fixture"]
    fn batch_import_fuel_matches_singles_and_exhausts_before_writes() {
        use wasmtime::{AsContextMut, component::Access};
        use wire::{HostNodeWithStore, HostWithStore};
        let group = fixture_group_with_fuel(true);
        let mut game = world();
        let declared = [Resolved::Member { owner: 0, field: 1 }];
        run(frame(&mut game), &declared, || {
            let root = View.root(0).unwrap();
            group.exports.load(|store| {
                macro_rules! invoke {
                    ($trait:ident::$method:ident $(, $arg:expr)*) => {
                        <View as $trait<State>>::$method(Access::new(store.as_context_mut(), |_| View) $(, $arg)*)
                    };
                }
                let writes = || vec![(borrow(&root), 1, vec![wire::Cell::B(true)]), (borrow(&root), 1, vec![wire::Cell::B(true)])];
                let initial = 1000;
                store.set_fuel(initial)?;
                for _ in 0..2 {
                    invoke!(HostNodeWithStore::set, borrow(&root), 1, vec![wire::Cell::B(true)])?.unwrap();
                }
                let single_set = initial - store.get_fuel()?;
                store.set_fuel(initial)?;
                invoke!(HostWithStore::set_many, writes())?;
                assert_eq!(
                    initial - store.get_fuel()?,
                    single_set,
                    "every entry pays its base charge, even refusals"
                );
                store.set_fuel(initial)?;
                for _ in 0..2 {
                    invoke!(HostNodeWithStore::get, borrow(&root), 1)?.unwrap();
                }
                let single_get = initial - store.get_fuel()?;
                store.set_fuel(initial)?;
                invoke!(HostWithStore::get_many, vec![(borrow(&root), 1), (borrow(&root), 1)])?;
                assert_eq!(initial - store.get_fuel()?, single_get);
                let pending = journal_cost();
                store.set_fuel(1)?;
                assert!(invoke!(HostWithStore::set_many, writes()).is_err());
                let bad = || (Resource::new_borrow(0), 0, Vec::new());
                assert!(invoke!(HostWithStore::set_many, vec![bad(), bad()]).is_err());
                store.set_fuel(initial)?;
                let many = || (0..13).map(|_| (borrow(&root), 1));
                assert!(invoke!(HostWithStore::get_many, many().collect()).is_err());
                assert!(invoke!(HostWithStore::set_many, many().map(|(n, f)| (n, f, vec![wire::Cell::B(true)])).collect()).is_err());
                assert_eq!(journal_cost(), pending);
                Ok(())
            }).unwrap();
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_binding_reads_writes_and_calls_the_table() {
        let mut game = world();
        run(
            frame(&mut game),
            &[
                Resolved::Thunk(0),
                Resolved::Member { owner: 0, field: 0 },
                Resolved::Member { owner: 0, field: 1 },
            ],
            || {
                let mut host = View;
                let root = host.root(0).unwrap();
                assert_eq!(
                    unflatten(host.get(borrow(&root), 1).unwrap()).unwrap(),
                    internals::Value::B(false)
                );
                host.set(borrow(&root), 1, vec![wire::Cell::B(true)])
                    .unwrap();
                assert_eq!(
                    unflatten(host.get(borrow(&root), 1).unwrap()).unwrap(),
                    internals::Value::B(true)
                );
                let players = host.field(borrow(&root), 0).unwrap();
                assert_eq!(host.len(borrow(&players)).unwrap(), 3);
                let wire::Reached::Node(first) = host.index(borrow(&players), 0).unwrap() else {
                    panic!("a node")
                };
                assert_eq!(
                    unflatten(host.read(borrow(&first)).unwrap()).unwrap(),
                    internals::Value::Rec(vec![internals::Value::S(10)])
                );
                let wire::Reached::Value(left) =
                    host.call(0, Some(borrow(&root)), vec![]).unwrap().result
                else {
                    panic!("a value")
                };
                assert_eq!(unflatten(left).unwrap(), internals::Value::U(2));
                assert_eq!(host.len(borrow(&players)).unwrap(), 2);
                assert!(matches!(
                    host.call(1, Some(borrow(&root)), vec![]),
                    Err(wire::Refusal::NotHere(_))
                ));
                Ok(())
            },
        )
        .unwrap();
        // Successful return applied the flag write before the thunk.
        let next = frame(&mut game);
        assert_eq!(
            next.get(&next.root(0).unwrap(), 1).unwrap(),
            internals::Value::B(true)
        );
    }

    fn assert_undeclared_field<T: std::fmt::Debug>(result: Result<T, wire::Refusal>) {
        match result {
            Err(wire::Refusal::NotHere(why)) => assert_eq!(why, "undeclared field"),
            other => panic!("expected an undeclared field refusal, got {other:?}"),
        }
    }

    #[test]
    fn receiver_fields_are_permissions_not_the_union_of_the_tables_members() {
        let mut game = world();
        let own = resolve(
            &TABLE,
            "own",
            &[("Game", "game-hash"), ("Game::kick", "field-kick-hash")],
        )
        .unwrap();
        let other = resolve(
            &TABLE,
            "other",
            &[("Game", "game-hash"), ("Game::flag", "flag-hash")],
        )
        .unwrap();
        run(frame(&mut game), &[], || {
            let root = View.root(0).unwrap();
            assert_undeclared_field(View.field(borrow(&root), 0));
            assert_undeclared_field(View.get(borrow(&root), 1));
            assert_undeclared_field(View.set(borrow(&root), 1, vec![wire::Cell::B(true)]));
            Ok(())
        })
        .unwrap();
        run(frame(&mut game), &own, || {
            let root = View.root(0).unwrap();
            let players = View.field(borrow(&root), 0).unwrap();
            let wire::Reached::Node(first) = View.index(borrow(&players), 0).unwrap() else {
                panic!("a player")
            };
            assert_eq!(
                unflatten(View.get(borrow(&first), 0).unwrap()).unwrap(),
                internals::Value::S(10)
            );
            View.set(borrow(&first), 0, vec![wire::Cell::S(17)])
                .unwrap();
            assert_eq!(
                unflatten(View.get(borrow(&first), 0).unwrap()).unwrap(),
                internals::Value::S(17)
            );
            assert_undeclared_field(View.get(borrow(&root), 1));
            assert_undeclared_field(View.set(borrow(&root), 1, vec![wire::Cell::B(true)]));
            Ok(())
        })
        .unwrap();
        run(frame(&mut game), &other, || {
            let root = View.root(0).unwrap();
            assert_eq!(
                unflatten(View.get(borrow(&root), 1).unwrap()).unwrap(),
                internals::Value::B(false)
            );
            View.set(borrow(&root), 1, vec![wire::Cell::B(true)])
                .unwrap();
            assert_undeclared_field(View.field(borrow(&root), 0));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn raw_record_fields_are_open_even_when_the_record_is_root_zero() {
        for receiver in [false, true] {
            let mut game = world();
            let root = crate::frame::Root {
                ptr: (&raw mut game).cast(),
                ty: 0,
                writable: true,
                receiver,
                born: None,
            };
            // SAFETY: this synthetic record has the fixture Game's layout and exclusive borrow.
            let frame = unsafe { Frame::new(&TABLE, vec![root]) };
            run(frame, &[], || {
                let root = View.root(0).unwrap();
                if receiver {
                    assert_undeclared_field(View.field(borrow(&root), 0));
                } else {
                    let players = View.field(borrow(&root), 0).unwrap();
                    assert_eq!(View.len(borrow(&players)).unwrap(), 3);
                    assert_eq!(
                        unflatten(View.get(borrow(&root), 1).unwrap()).unwrap(),
                        internals::Value::B(false)
                    );
                    View.set(borrow(&root), 1, vec![wire::Cell::B(true)])
                        .unwrap();
                    assert_eq!(
                        unflatten(View.get(borrow(&root), 1).unwrap()).unwrap(),
                        internals::Value::B(true)
                    );
                }
                Ok(())
            })
            .unwrap();
        }
    }

    #[test]
    fn a_subtree_node_used_as_a_call_receiver_becomes_restricted() {
        let mut game = world();
        run(
            frame(&mut game),
            &[Resolved::Member { owner: 0, field: 0 }, Resolved::Thunk(3)],
            || {
                let root = View.root(0).unwrap();
                let players = View.field(borrow(&root), 0).unwrap();
                let wire::Reached::Node(first) = View.index(borrow(&players), 0).unwrap() else {
                    panic!("a player")
                };
                assert_eq!(
                    unflatten(View.read(borrow(&first)).unwrap()).unwrap(),
                    internals::Value::Rec(vec![internals::Value::S(10)])
                );
                View.call(1, Some(borrow(&first)), vec![]).unwrap();
                assert_undeclared_field(View.get(borrow(&first), 0));
                assert_undeclared_field(View.set(borrow(&first), 0, vec![wire::Cell::S(0)]));
                assert_undeclared_field(View.read(borrow(&first)));
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn a_returned_reference_has_an_unrestricted_subtree() {
        let mut game = world();
        run(frame(&mut game), &[Resolved::Thunk(1)], || {
            let root = View.root(0).unwrap();
            let wire::Reached::Node(first) =
                View.call(0, Some(borrow(&root)), vec![]).unwrap().result
            else {
                panic!("a player")
            };
            assert_eq!(
                unflatten(View.get(borrow(&first), 0).unwrap()).unwrap(),
                internals::Value::S(10)
            );
            View.set(borrow(&first), 0, vec![wire::Cell::S(17)])
                .unwrap();
            assert_eq!(
                unflatten(View.get(borrow(&first), 0).unwrap()).unwrap(),
                internals::Value::S(17)
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn no_frame_and_a_node_kept_into_the_next_call_are_gone() {
        let mut host = View;
        assert!(
            matches!(host.root(0), Err(wire::Refusal::Gone(why)) if why == "no call is running")
        );
        assert!(matches!(
            host.set(Resource::new_borrow(0), 0, vec![]),
            Err(wire::Refusal::Gone(_))
        ));
        let mut game = world();
        let old = run(frame(&mut game), &[], || Ok(host.root(0).unwrap())).unwrap();
        assert!(matches!(
            host.get(borrow(&old), 1),
            Err(wire::Refusal::Gone(_))
        ));
        run(frame(&mut game), &[], || {
            let new = host.root(0).unwrap();
            assert_ne!(old.rep(), new.rep());
            assert!(matches!(
                host.get(borrow(&old), 1),
                Err(wire::Refusal::Gone(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_trap_discards_writes_and_clears_the_scope() {
        let mut game = world();
        let result: wasmtime::Result<()> = run(
            frame(&mut game),
            &[Resolved::Member { owner: 0, field: 1 }],
            || {
                let root = View.root(0).unwrap();
                View.set(borrow(&root), 1, vec![wire::Cell::B(true)])
                    .unwrap();
                Err(wasmtime::Error::msg("guest trapped"))
            },
        );
        assert!(result.is_err());
        assert!(matches!(View.root(0), Err(wire::Refusal::Gone(_))));
        let next = frame(&mut game);
        assert_eq!(
            next.get(&next.root(0).unwrap(), 1).unwrap(),
            internals::Value::B(false)
        );
    }

    #[test]
    fn import_fuel_is_debited_and_exhaustion_is_a_trap() {
        let mut config = crate::RuntimeConfig::default();
        config.budgets.fuel = Some(Default::default());
        let engine = crate::engine(&config).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        store.set_fuel(9).unwrap();
        charge(&mut store, 4).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 5);
        let refusal = charge(&mut store, 6).unwrap_err();
        assert_eq!(refusal.to_string(), "internals fuel exhausted");
        assert_eq!(store.get_fuel().unwrap(), 5);
        let engine = crate::engine(&crate::RuntimeConfig::default()).unwrap();
        let mut unmetered = wasmtime::Store::new(&engine, ());
        charge(&mut unmetered, 1000).unwrap();
    }

    #[test]
    fn a_worker_thread_cannot_use_the_callers_internals_frame() {
        let mut game = world();
        run(frame(&mut game), &[], || {
            std::thread::spawn(|| {
                assert!(matches!(View.root(0), Err(wire::Refusal::Gone(_))));
                assert!(matches!(
                    View.get(Resource::new_borrow(0), 0),
                    Err(wire::Refusal::Gone(_))
                ));
            })
            .join()
            .unwrap();
            assert!(
                View.root(0).is_ok(),
                "the owning thread still has its frame"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_thunk_cannot_reenter_its_listeners_view() {
        unsafe fn reenter(
            _: Option<*mut ()>,
            _: Vec<internals::Arg>,
        ) -> Result<internals::CallReply, internals::Refusal> {
            assert!(matches!(View.root(0), Err(wire::Refusal::Gone(_))));
            Ok(internals::CallReply::new(internals::Reply::Value(
                internals::Value::None,
            )))
        }
        static TABLE: Table = Table {
            types: &[],
            thunks: &[internals::Thunk {
                path: "reenter",
                hash: "hash",
                receiver: None,
                args: &[],
                writes: 0,
                call: reenter,
            }],
            places: &[],
            members: &[],
            native_only: &[],
        };
        // SAFETY: the test has no game roots; its thunk only checks the import's refusal.
        let frame = unsafe { Frame::new(&TABLE, vec![]) };
        run(frame, &[Resolved::Thunk(0)], || {
            assert!(matches!(
                View.call(0, None, vec![]),
                Ok(wire::CallReply {
                    result: wire::Reached::Value(_),
                    ..
                })
            ));
            // The mask restored the active frame, rather than leaving the imports outside it.
            assert!(matches!(
                View.call(1, None, vec![]),
                Err(wire::Refusal::NotHere(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_declared_field_checks_its_owner_and_disambiguates_a_method_path() {
        assert_eq!(
            resolve(&TABLE, "test", &[("Game::flag", "flag-hash")]),
            Err(crate::Refusal::ChangedInternal {
                path: "Game".into()
            })
        );
        assert_eq!(
            resolve(
                &TABLE,
                "test",
                &[
                    ("Game", "game-hash"),
                    ("Game::flag", "flag-hash"),
                    ("Game::kick", "field-kick-hash"),
                    ("Game::kick", "kick-hash")
                ]
            ),
            Ok(vec![
                Resolved::Type(0),
                Resolved::Member { owner: 0, field: 1 },
                Resolved::Member { owner: 0, field: 0 },
                Resolved::Thunk(0)
            ])
        );
    }

    #[test]
    fn native_only_refusal_names_the_path_even_with_no_generated_entries() {
        let table = Table {
            native_only: &["GPU::draw"],
            ..Table::EMPTY
        };
        let refusal = resolve(&table, "test", &[("GPU::draw", "hash")]).unwrap_err();
        assert_eq!(
            refusal,
            crate::Refusal::MissingInternal {
                path: "GPU::draw".into(),
                file: "crates/horizon-mod-api-gen/wasm-internals/test.txt".into(),
                native_only: true,
            }
        );
        assert_eq!(refusal.to_string(), "native only: GPU::draw");
        assert_eq!(
            resolve(&table, "test", &[("Game::unlisted", "hash")]),
            Err(crate::Refusal::MissingInternal {
                path: "Game::unlisted".into(),
                file: "crates/horizon-mod-api-gen/wasm-internals/test.txt".into(),
                native_only: false,
            })
        );
    }

    #[test]
    fn declared_paths_resolve_by_path_then_hash() {
        assert_eq!(
            resolve(
                &TABLE,
                "test",
                &[("Game", "game-hash"), ("Game::kick", "kick-hash")]
            ),
            Ok(vec![Resolved::Type(0), Resolved::Thunk(0)])
        );
        assert_eq!(
            resolve(&TABLE, "test", &[("Game", "old")]),
            Err(crate::Refusal::ChangedInternal {
                path: "Game".into()
            })
        );
        for path in ["Missing", "Vec<Player>"] {
            assert_eq!(
                resolve(&TABLE, "test", &[(path, "")]),
                Err(crate::Refusal::MissingInternal {
                    path: path.into(),
                    file: "crates/horizon-mod-api-gen/wasm-internals/test.txt".into(),
                    native_only: false,
                })
            );
        }
        assert_eq!(
            resolve(&Table::EMPTY, "test", &[("Game", "game-hash")]),
            Err(crate::Refusal::NoInternalsTable)
        );
        assert_eq!(resolve(&Table::EMPTY, "test", &[]), Ok(vec![]));
    }
}
