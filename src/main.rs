//! SigmaOS-ipc — thin CLI entry point.
//!
//! The component library lives in `src/lib.rs`; this binary is a minimal
//! entry that reports the component is present. Real behaviour is in the
//! library crate and exercised by its unit/integration tests.

fn main() {
    println!("SigmaOS-ipc: ok");
}
