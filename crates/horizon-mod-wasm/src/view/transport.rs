//! Stock canonical list cursors on input, and store-owned buffers retained until lowering ends.
use super::*;
use std::sync::Arc;
use wasmtime::component::WasmList;
use wasmtime::{AsContextMut, StoreContextMut};

type Get = Result<Arc<Vec<wire::Cell>>, wire::Refusal>;
type Read = (Resource<wire::Node>, u32);
type Write = (Resource<wire::Node>, u32, WasmList<wire::Cell>);

#[derive(Default)]
pub(super) struct Buffers {
    cells: Vec<Arc<Vec<wire::Cell>>>,
    pending: Vec<internals::Value>,
    gets: Arc<Vec<Get>>,
    sets: Arc<Vec<Result<(), wire::Refusal>>>,
    pub(super) inputs: Vec<Vec<wire::Cell>>,
    requests: Vec<Read>,
    refused: Vec<bool>,
}

impl Buffers {
    fn begin(&mut self) {
        Arc::get_mut(&mut self.gets).unwrap().clear();
        Arc::get_mut(&mut self.sets).unwrap().clear();
        self.requests.clear();
        self.refused.clear();
    }

    pub(super) fn clear_inputs(&mut self) {
        for input in &mut self.inputs {
            input.clear();
            input.shrink_to(1024);
        }
    }

    fn encode(&mut self, index: usize, value: internals::Value) -> Get {
        crate::frame::value_size(&value)?;
        while self.cells.len() <= index {
            self.cells.push(Arc::new(Vec::new()));
        }
        let cells = Arc::get_mut(&mut self.cells[index]).unwrap();
        cells.clear();
        internals::flatten_into(value, cells, &mut self.pending);
        Ok(self.cells[index].clone())
    }

    fn input(
        &mut self,
        store: &mut StoreContextMut<'_, State>,
        cells: WasmList<wire::Cell>,
        index: usize,
    ) -> wasmtime::Result<bool> {
        while self.inputs.len() <= index {
            self.inputs.push(Vec::new());
        }
        if cells.len() > 65_536 {
            return Ok(false);
        }
        let input = &mut self.inputs[index];
        input.clear();
        let mut bytes = 0usize;
        for index in 0..cells.len() {
            let cell = cells.get(store.as_context_mut(), index).unwrap()?;
            if let wire::Cell::Str(value) = &cell {
                let next = bytes.saturating_add(value.len());
                charge(
                    store.as_context_mut(),
                    next.div_ceil(64) - bytes.div_ceil(64),
                )?;
                bytes = next;
                if bytes > 1_048_576 {
                    return Ok(false);
                }
            }
            input.push(cell);
        }
        Ok(true)
    }
}

fn get_many(
    store: StoreContextMut<'_, State>,
    reads: WasmList<Read>,
) -> wasmtime::Result<Arc<Vec<Get>>> {
    bounded_batch(reads.len())?;
    State::with_buffer::<Buffers, _>(store, |buffers, mut store| {
        contain(|| {
            buffers.begin();
            let mut cost = reads.len().max(1);
            for read in reads.iter(store.as_context_mut())? {
                let (node, field) = read?;
                cost = cost
                    .saturating_add(node_cost(&node))
                    .saturating_add(journal_cost());
                buffers.requests.push((node, field));
            }
            charge(&mut store, cost)?;
            let mut output = 0usize;
            for index in 0..buffers.requests.len() {
                let (node, field) = &buffers.requests[index];
                let value = View
                    .get_value(node, *field)
                    .and_then(|v| buffers.encode(index, v));
                if let Ok(cells) = &value {
                    output = output.saturating_add(wire_cost(cells));
                }
                Arc::get_mut(&mut buffers.gets).unwrap().push(value);
            }
            charge(&mut store, output)?;
            Ok(buffers.gets.clone())
        })
    })
}

fn set_many(
    store: StoreContextMut<'_, State>,
    writes: WasmList<Write>,
) -> wasmtime::Result<Arc<Vec<Result<(), wire::Refusal>>>> {
    bounded_batch(writes.len())?;
    State::with_buffer::<Buffers, _>(store, |buffers, mut store| {
        let result = contain(|| {
            #[cfg(test)]
            let fuel_before = store.get_fuel().ok();
            buffers.begin();
            charge(&mut store, writes.len().max(1))?;
            for index in 0..writes.len() {
                let (node, field, cells) = writes.get(store.as_context_mut(), index).unwrap()?;
                charge(&mut store, node_cost(&node).saturating_add(cells.len()))?;
                let accepted = buffers.input(&mut store, cells, index)?;
                buffers.refused.push(!accepted);
                buffers.requests.push((node, field));
            }
            for (index, (node, field)) in buffers.requests.drain(..).enumerate() {
                let value = if buffers.refused[index] {
                    Err(wire::Refusal::NotHere("internals budget".into()))
                } else {
                    View.set_buffer(&node, field, &mut buffers.inputs[index])
                };
                Arc::get_mut(&mut buffers.sets).unwrap().push(value);
            }
            #[cfg(test)]
            if let Some(before) = fuel_before {
                tests::FUEL_DEBITS
                    .with_borrow_mut(|debits| debits.push(before - store.get_fuel().unwrap()));
            }
            Ok(buffers.sets.clone())
        });
        buffers.clear_inputs();
        result
    })
}

pub(super) fn add_to_linker(linker: &mut Linker<State>) -> wasmtime::Result<()> {
    linker.allow_shadowing(true);
    let mut interface = linker.instance("horizon:mod-internals/view@0.1.0")?;
    interface.func_wrap("get-many", |store, (reads,)| Ok((get_many(store, reads)?,)))?;
    interface.func_wrap("set-many", |store, (writes,)| {
        Ok((set_many(store, writes)?,))
    })?;
    linker.allow_shadowing(false);
    Ok(())
}
