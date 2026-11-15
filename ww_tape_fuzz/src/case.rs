//! One case of a fuzz target, as a test or a tool runs it: its panic and any allocation over [ALLOC_MAX] are the
//! outcome of the case, not of the process. A binary that runs cases declares [Capped] its global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Largest single allocation a case may make: 128 MiB, the fuzz recipes' `-malloc_limit_mb`.
pub const ALLOC_MAX: usize = 128 << 20;
/// Allocations past this are refused outright, which aborts the process; up to it an allocation over [ALLOC_MAX] is
/// made and only reported.
const ALLOC_REFUSED: usize = 1 << 30;

/// The largest allocation over [ALLOC_MAX] since it was last taken, 0 for none.
static OVER: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, noting every single allocation over [ALLOC_MAX] for [run]; a binary declares it with
/// `#[global_allocator] static ALLOCATOR: Capped = Capped;`.
pub struct Capped;

impl Capped {
    fn note(size: usize) -> bool {
        if size > ALLOC_MAX {
            OVER.fetch_max(size, Ordering::Relaxed);
        }
        size <= ALLOC_REFUSED
    }
}

// SAFETY: every call goes to System with the caller's layout and pointer unchanged; a refused allocation returns null,
// which GlobalAlloc allows.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !Self::note(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout, as GlobalAlloc::alloc requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !Self::note(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout, as GlobalAlloc::alloc_zeroed requires.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !Self::note(new_size) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's pointer, layout and size, as GlobalAlloc::realloc requires.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's pointer and layout, as GlobalAlloc::dealloc requires.
        unsafe { System.dealloc(ptr, layout) }
    }
}

thread_local! {
    /// Whether [run] is running a case on this thread: its panics are kept, not printed.
    static IN_CASE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    /// Where and why the last panic on this thread happened.
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Keep the location and message of each panic in a case for [run], instead of printing them.
fn quiet_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !IN_CASE.with(core::cell::Cell::get) {
                return default(info);
            }
            let at = info
                .location()
                .map_or_else(String::new, |l| format!("{}:{}", l.file(), l.line()));
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            LAST_PANIC.with(|p| *p.borrow_mut() = Some((at, msg)));
        }));
    });
}

/// How one case ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// A panic: its location (`file:line`) and message.
    Panic(String, String),
    /// A single allocation over [ALLOC_MAX]: its size.
    Alloc(usize),
}

/// Run `target` on `data`; the wall-clock time it took is for reports only.
pub fn run(target: fn(&[u8]), data: &[u8]) -> (Outcome, Duration) {
    quiet_panics();
    OVER.store(0, Ordering::Relaxed);
    let start = Instant::now();
    IN_CASE.with(|c| c.set(true));
    let r = catch_unwind(AssertUnwindSafe(|| target(data)));
    IN_CASE.with(|c| c.set(false));
    let took = start.elapsed();
    let over = OVER.swap(0, Ordering::Relaxed);
    let outcome = match r {
        Err(_) => {
            let (at, msg) = LAST_PANIC
                .with(|p| p.borrow_mut().take())
                .unwrap_or_default();
            Outcome::Panic(at, msg)
        }
        Ok(()) if over > 0 => Outcome::Alloc(over),
        Ok(()) => Outcome::Ok,
    };
    (outcome, took)
}

impl Outcome {
    /// Where the case failed: the panic's `file:line`, `alloc` for an allocation over the cap; None when it passed.
    pub fn place(&self) -> Option<&str> {
        match self {
            Outcome::Ok => None,
            Outcome::Panic(at, _) => Some(at),
            Outcome::Alloc(_) => Some("alloc"),
        }
    }
}

/// Passes of [minimize] over an input at most.
const MINIMIZE_PASSES_MAX: usize = 8;

/// `input` made as short and as plain as it gets while `target` still fails at the same place: cut from the end,
/// bytes set to 0, bytes removed. An input that passes comes back as it is.
pub fn minimize(target: fn(&[u8]), input: &[u8]) -> Vec<u8> {
    let Some(place) = run(target, input).0.place().map(str::to_owned) else {
        return input.to_vec();
    };
    let fails = |data: &[u8]| run(target, data).0.place() == Some(place.as_str());
    let mut best = input.to_vec();
    for _ in 0..MINIMIZE_PASSES_MAX {
        let before = best.clone();
        while !best.is_empty() && fails(&best[..best.len() - 1]) {
            best.pop();
        }
        for i in 0..best.len() {
            if best[i] != 0 {
                let mut t = best.clone();
                t[i] = 0;
                if fails(&t) {
                    best = t;
                }
            }
        }
        let mut i = 0;
        while i < best.len() {
            let mut t = best.clone();
            t.remove(i);
            if fails(&t) {
                best = t;
            } else {
                i += 1;
            }
        }
        if best == before {
            break;
        }
    }
    best
}
