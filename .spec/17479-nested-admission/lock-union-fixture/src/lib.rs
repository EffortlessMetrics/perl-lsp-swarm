use parking_lot::{Mutex as PlMutex, MutexGuard as PlMutexGuard, RwLock as PlRwLock};
use std::sync::{Arc, Mutex, RwLock};

pub fn discards_std_guards(dropped_std_mutex: &Mutex<u32>, dropped_std_rwlock: &RwLock<u32>) {
    let _ = dropped_std_mutex.lock();
    let _ = dropped_std_rwlock.read();
}

pub fn discards_parking_lot_guards(dropped_pl_mutex: &PlMutex<u32>, dropped_pl_rwlock: &PlRwLock<u32>) {
    let _ = dropped_pl_mutex.lock();
    let _ = dropped_pl_rwlock.write();
}

pub fn discards_arc_guards(dropped_arc_mutex: &Arc<PlMutex<u32>>, dropped_arc_rwlock: &Arc<PlRwLock<u32>>) {
    let _ = dropped_arc_mutex.lock_arc();
    let _ = dropped_arc_rwlock.write_arc();
}

pub fn discards_by_explicit_drop(dropped_via_std_drop: &Mutex<u32>, dropped_via_pl_drop: &PlMutex<u32>) {
    drop(dropped_via_std_drop.lock());
    drop(dropped_via_pl_drop.lock());
}

pub fn discards_mapped_guard(dropped_mapped_pl: &PlMutex<(u32, u32)>) {
    let dropped_mapped_pl_guard = dropped_mapped_pl.lock();
    let _ = PlMutexGuard::map(dropped_mapped_pl_guard, |value| &mut value.0);
}

pub fn holds_mapped_guard(held_mapped_pl: &PlMutex<(u32, u32)>) -> u32 {
    let held_mapped_pl_guard = held_mapped_pl.lock();
    let mut mapped = PlMutexGuard::map(held_mapped_pl_guard, |value| &mut value.0);
    *mapped = mapped.wrapping_add(1);
    *mapped
}

pub fn holds_arc_guards(held_arc_mutex: &Arc<PlMutex<u32>>, held_arc_rwlock: &Arc<PlRwLock<u32>>) -> u32 {
    let mut guard = held_arc_mutex.lock_arc();
    *guard = guard.wrapping_add(1);
    let mut writer = held_arc_rwlock.write_arc();
    *writer = guard.wrapping_add(1);
    *writer
}

pub fn holds_std_guards(held_std_mutex: &Mutex<u32>, held_std_rwlock: &RwLock<u32>) -> Option<u32> {
    let mut guard = held_std_mutex.lock().ok()?;
    *guard = guard.wrapping_add(1);
    let reader = held_std_rwlock.read().ok()?;
    Some(guard.wrapping_add(*reader))
}

pub fn holds_parking_lot_guards(held_pl_mutex: &PlMutex<u32>, held_pl_rwlock: &PlRwLock<u32>) -> u32 {
    let mut guard = held_pl_mutex.lock();
    *guard = guard.wrapping_add(1);
    let mut writer = held_pl_rwlock.write();
    *writer = guard.wrapping_add(1);
    *writer
}
