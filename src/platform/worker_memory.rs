//! Process-local allocation admission for the experimental Boa worker profile.
//!
//! Counts complete requests to the system allocator, including this wrapper's
//! header/alignment, after activation in a fresh restricted child. This is not
//! GC live-heap accounting. Limit failure exits the child without unwinding or
//! running JS finalizers; the parent rejects the incomplete transaction.
use mg_butane::modern::WorkerMemory;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const OUTSTANDING_LIMIT: u64 = mg_butane::modern::WORKER_OUTSTANDING_LIMIT;
pub const CUMULATIVE_LIMIT: u64 = mg_butane::modern::WORKER_CUMULATIVE_LIMIT;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);
static COUNT: AtomicU64 = AtomicU64::new(0);

/// Install as the executable allocator, not in libraries or measurement examples.
pub struct WorkerAllocator;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    tracked: u64,
}

fn expanded(layout: Layout) -> Option<(Layout, usize)> {
    let alignment = layout.align().max(std::mem::align_of::<Header>());
    let offset = std::mem::size_of::<Header>().checked_add(alignment - 1)? & !(alignment - 1);
    let size = offset.checked_add(layout.size())?;
    Some((Layout::from_size_align(size, alignment).ok()?, offset))
}

fn rejected() -> ! {
    // No allocation, formatting, panic/unwind, or script callback is permitted
    // on this path. Only stderr and process exit are available after seccomp.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    unsafe {
        let message = b"Boa worker requested-allocation limit reached\n";
        libc::write(2, message.as_ptr().cast(), message.len());
        libc::_exit(75);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    std::process::abort()
}

fn reserve(counter: &AtomicU64, amount: u64, limit: u64) -> u64 {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
            old.checked_add(amount).filter(|next| *next <= limit)
        })
        .map(|old| old + amount)
        .unwrap_or_else(|_| rejected())
}

// SAFETY: every allocation has its own aligned header. Deallocation/reallocation
// use the original Layout to recover precisely the System allocation. We do not
// allocate, panic, or acquire Rust locks in the allocator. Atomics are lock-free
// on the supported x86_64 target; production activation precedes engine creation.
unsafe impl GlobalAlloc for WorkerAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some((whole, offset)) = expanded(layout) else {
            if ACTIVE.load(Ordering::SeqCst) {
                rejected();
            }
            return std::ptr::null_mut();
        };
        let tracked = ACTIVE.load(Ordering::SeqCst);
        if tracked {
            let amount = whole.size() as u64;
            reserve(&TOTAL, amount, CUMULATIVE_LIMIT);
            let live = reserve(&LIVE, amount, OUTSTANDING_LIMIT);
            PEAK.fetch_max(live, Ordering::SeqCst);
            COUNT.fetch_add(1, Ordering::SeqCst);
        }
        let base = unsafe { System.alloc(whole) };
        if base.is_null() {
            if tracked {
                rejected();
            }
            return base;
        }
        let result = unsafe { base.add(offset) };
        unsafe {
            result
                .sub(std::mem::size_of::<Header>())
                .cast::<Header>()
                .write(Header {
                    tracked: u64::from(tracked),
                });
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let Some((whole, offset)) = expanded(layout) else {
            rejected();
        };
        let header = unsafe {
            pointer
                .sub(std::mem::size_of::<Header>())
                .cast::<Header>()
                .read()
        };
        if header.tracked != 0 {
            LIVE.fetch_sub(whole.size() as u64, Ordering::SeqCst);
        }
        unsafe { System.dealloc(pointer.sub(offset), whole) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            if ACTIVE.load(Ordering::SeqCst) {
                rejected();
            }
            return std::ptr::null_mut();
        };
        // A real new allocation and copy: count both buffers at the peak, never
        // refund cumulative admission, and retain the original on allocation failure.
        let new = unsafe { self.alloc(new_layout) };
        if !new.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(pointer, new, layout.size().min(new_size));
                self.dealloc(pointer, layout);
            }
        }
        new
    }
}

/// Called once, only after confinement in a fresh child. Never reset on input.
pub fn activate() {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        rejected();
    }
}

pub fn report() -> WorkerMemory {
    WorkerMemory {
        outstanding_bytes: LIVE.load(Ordering::SeqCst),
        peak_bytes: PEAK.load(Ordering::SeqCst),
        cumulative_bytes: TOTAL.load(Ordering::SeqCst),
        allocations: COUNT.load(Ordering::SeqCst),
        outstanding_limit_bytes: OUTSTANDING_LIMIT,
        cumulative_limit_bytes: CUMULATIVE_LIMIT,
    }
}

/// Owned-child probes, reached through the existing isolation selftest endpoint.
pub fn probe(mode: &str) -> bool {
    match mode {
        "allocation-live" => {
            std::hint::black_box(vec![0u8; OUTSTANDING_LIMIT as usize + 1]);
            panic!("Outstanding allocation admission unexpectedly succeeded")
        }
        "allocation-cumulative" => {
            for _ in 0..128 {
                let bytes = vec![1u8; 1024 * 1024];
                std::hint::black_box(&bytes);
            }
            panic!("Cumulative allocation admission unexpectedly succeeded")
        }
        "allocation-alignment" => {
            let before = report();
            for alignment in [1, 2, 8, 16, 64, 4096] {
                let layout = Layout::from_size_align(127, alignment).unwrap();
                unsafe {
                    let p = WorkerAllocator.alloc(layout);
                    assert!(!p.is_null());
                    assert_eq!(p as usize % alignment, 0);
                    p.write_bytes(0x93, 127);
                    let grown = WorkerAllocator.realloc(p, layout, 8193);
                    assert!(!grown.is_null());
                    assert!(
                        std::slice::from_raw_parts(grown, 127)
                            .iter()
                            .all(|b| *b == 0x93)
                    );
                    WorkerAllocator
                        .dealloc(grown, Layout::from_size_align(8193, alignment).unwrap());
                }
            }
            let after = report();
            assert_eq!(after.outstanding_bytes, before.outstanding_bytes);
            assert!(after.cumulative_bytes > before.cumulative_bytes);
            assert!(after.allocations >= before.allocations + 12);
            assert!(after.is_valid_after(&before));
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_header_alignment_and_reallocation_preserve_bytes() {
        for alignment in [1, 2, 8, 16, 64, 4096] {
            let layout = Layout::from_size_align(127, alignment).unwrap();
            let (whole, offset) = expanded(layout).unwrap();
            assert_eq!(offset % alignment, 0);
            assert!(offset >= std::mem::size_of::<Header>());
            assert_eq!(whole.size(), offset + 127);
            unsafe {
                let pointer = WorkerAllocator.alloc(layout);
                assert!(!pointer.is_null());
                assert_eq!(pointer as usize % alignment, 0);
                pointer.write_bytes(0xa5, 127);
                let grown = WorkerAllocator.realloc(pointer, layout, 4097);
                assert!(!grown.is_null());
                assert!(
                    std::slice::from_raw_parts(grown, 127)
                        .iter()
                        .all(|b| *b == 0xa5)
                );
                WorkerAllocator.dealloc(grown, Layout::from_size_align(4097, alignment).unwrap());
            }
        }
    }
}
