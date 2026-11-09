//! mimalloc replaces the platform heap: a compile makes millions of small
//! allocations, which the Windows default heap serves slowly.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
