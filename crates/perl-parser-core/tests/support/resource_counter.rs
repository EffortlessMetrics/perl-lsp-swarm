//! Thread-local allocator observations for parser resource regressions.
//!
//! Counts allocator requests, including the full new size of reallocations.
//! This measures requested bytes, not resident memory or allocator overhead.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default)]
pub struct AllocationWork {
    pub calls: u64,
    pub requested_bytes: u64,
}

thread_local! {
    static WORK: Cell<Option<AllocationWork>> = const { Cell::new(None) };
}

pub struct CountingAllocator;

fn record(size: usize) {
    let _ = WORK.try_with(|cell| {
        if let Some(mut work) = cell.get() {
            work.calls += 1;
            work.requested_bytes += size as u64;
            cell.set(Some(work));
        }
    });
}

// SAFETY: every request is forwarded unchanged to System; observations neither
// allocate nor alter pointers, layouts, or allocator ownership.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the GlobalAlloc layout contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the GlobalAlloc layout contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        // SAFETY: System owns ptr, and the caller supplies the realloc contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: System owns ptr with the original caller-supplied layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, AllocationWork) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            WORK.with(|cell| cell.set(None));
        }
    }
    WORK.with(|cell| {
        assert!(cell.get().is_none(), "allocation observations cannot nest");
        cell.set(Some(AllocationWork::default()));
    });
    let reset = Reset;
    let result = operation();
    let work = WORK.with(|cell| cell.replace(None)).unwrap_or_default();
    drop(reset);
    (result, work)
}
