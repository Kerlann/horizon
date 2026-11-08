//! Temporary baseline instrumentation, not production code.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Instant,
};
const N: usize = 17;
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static TIMING: Cell<bool> = const { Cell::new(true) };
    static PHASE: Cell<usize> = const { Cell::new(0) };
    static LAST: Cell<Option<Instant>> = const { Cell::new(None) };
    static TIME: Cell<[u64; N]> = const { Cell::new([0; N]) };
    static ALLOCS: Cell<[u64; N]> = const { Cell::new([0; N]) };
    static BYTES: Cell<[u64; N]> = const { Cell::new([0; N]) };
}
pub fn mark(next: usize) {
    if !ACTIVE.get() {
        return;
    }
    if TIMING.get() {
        let now = Instant::now();
        if let Some(last) = LAST.replace(Some(now)) {
            let mut times = TIME.get();
            times[PHASE.get()] += now.duration_since(last).as_nanos() as u64;
            TIME.set(times);
        }
    }
    PHASE.set(next);
}
pub struct Scope(usize);
pub fn enter(next: usize) -> Scope {
    let previous = PHASE.get();
    mark(next);
    Scope(previous)
}
impl Drop for Scope {
    fn drop(&mut self) {
        mark(self.0);
    }
}
pub fn start() {
    TIMING.set(true);
    TIME.set([0; N]);
    ALLOCS.set([0; N]);
    BYTES.set([0; N]);
    PHASE.set(0);
    LAST.set(Some(Instant::now()));
    ACTIVE.set(true);
}
/// Count allocations by phase without reading clocks at phase transitions.
pub fn start_allocations() {
    start();
    TIMING.set(false);
    LAST.set(None);
}
pub fn finish() -> ([u64; N], [u64; N], [u64; N]) {
    mark(0);
    ACTIVE.set(false);
    (TIME.get(), ALLOCS.get(), BYTES.get())
}
fn allocated(bytes: usize) {
    if !ACTIVE.get() {
        return;
    }
    let phase = PHASE.get();
    let mut counts = ALLOCS.get();
    counts[phase] += 1;
    ALLOCS.set(counts);
    let mut sizes = BYTES.get();
    sizes[phase] += bytes as u64;
    BYTES.set(sizes);
}
pub struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocated(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
