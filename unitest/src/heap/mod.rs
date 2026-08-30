// ============================================================
// unitest::heap —— 对应 kernel/src/mm/heap
// ============================================================

#[path = "../../../kernel/src/mm/heap/slub.rs"]
pub mod slub;

#[path = "../../../kernel/src/mm/heap/slab_cache.rs"]
pub mod slab_cache;

#[path = "../../../kernel/src/mm/heap/kmalloc.rs"]
pub mod kmalloc;
