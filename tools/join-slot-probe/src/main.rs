//! Run with `cargo run --manifest-path tools/join-slot-probe/Cargo.toml --profile fast`.
use join_slot_probe::join_seq::{JoinList, JoinSeq, Joined};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[derive(Clone)]
struct Entity {
    join: JoinSeq,
    // Not a boat/mob model: keep removal's element size explicit (64 bytes).
    payload: [u64; 7],
}
impl Joined for Entity {
    fn join_seq(&self) -> JoinSeq {
        self.join
    }
    fn set_join_seq(&mut self, join: JoinSeq) {
        self.join = join;
    }
}
fn old_find(list: &[Entity], join: JoinSeq) -> Option<usize> {
    match list.binary_search_by_key(&join, Joined::join_seq) {
        Ok(i) => Some(i),
        Err(_) => list.iter().position(|e| e.join_seq() == join),
    }
}
fn time(mut action: impl FnMut(), reps: usize) -> (f64, usize) {
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..reps {
        action();
    }
    let ns = start.elapsed().as_nanos() as f64 / reps as f64;
    (ns, ALLOCATIONS.load(Ordering::Relaxed) - allocations)
}
fn lookup(label: &str, list: &JoinList<Entity>) {
    let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
    for &join in &joins {
        assert_eq!(list.join_slot(join), old_find(list, join));
    }
    let mut i = 0;
    let (indexed, _) = time(
        || {
            i = (i + 97) % joins.len();
            black_box(list.join_slot(black_box(joins[i])));
        },
        200_000,
    );
    let (old, _) = time(
        || {
            i = (i + 97) % joins.len();
            black_box(old_find(black_box(list), black_box(joins[i])));
        },
        200_000,
    );
    println!("lookup,{},{label},{indexed:.2},{old:.2}", list.len());
}
mod mixed;

fn main() {
    if std::env::args().any(|a| a == "--mixed") {
        mixed::run();
        return;
    }
    println!(
        "logical cores: {}; Entity bytes: {}",
        std::thread::available_parallelism().unwrap(),
        size_of::<Entity>()
    );
    println!("operation,N,phase,indexed ns,old ns");
    for n in [100, 1_000, 10_000] {
        let mut list: JoinList<_> = (0..n)
            .map(|_| Entity {
                join: JoinSeq::UNJOINED,
                payload: [0; 7],
            })
            .collect();
        lookup("push", &list);
        let removed = list.remove(n / 2);
        assert_eq!(list.join_slot(removed.join), None);
        lookup("ordered remove", &list);
        list.swap(0, n - 2);
        lookup("swap", &list);
        let mut taken = std::mem::take(&mut list);
        list.push(Entity {
            join: JoinSeq::UNJOINED,
            payload: [0; 7],
        });
        taken.append(&mut list);
        list = taken;
        lookup("take restore", &list);
        for i in 0..n {
            list.swap(i, (i * 7919 + 17) % n);
        }
        lookup("out of order", &list);
        let baseline = list.to_vec();
        let mut indexed_remove = 0.0;
        let mut old_remove = 0.0;
        // Setup and destruction are outside each timed removal.
        for _ in 0..200 {
            let mut copy = list.clone();
            indexed_remove += time(
                || {
                    black_box(copy.remove(n / 2));
                },
                1,
            )
            .0;
            let mut copy = baseline.clone();
            old_remove += time(
                || {
                    black_box(copy.remove(n / 2));
                },
                1,
            )
            .0;
        }
        println!(
            "remove,{n},middle,{:.2},{:.2}",
            indexed_remove / 200.0,
            old_remove / 200.0
        );
        let mut old = baseline;
        let (idle, allocations) = time(
            || {
                list.retain(|e| black_box(e.payload[0]) == 0);
                black_box(&list);
            },
            10_000,
        );
        let (old_idle, old_allocations) = time(
            || {
                old.retain(|e| black_box(e.payload[0]) == 0);
                black_box(&old);
            },
            10_000,
        );
        assert_eq!(allocations, 0);
        assert_eq!(old_allocations, 0);
        println!(
            "idle retain,{n},allocations {allocations}/{old_allocations},{idle:.2},{old_idle:.2}"
        );
    }
}
