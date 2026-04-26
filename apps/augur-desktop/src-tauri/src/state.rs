//! Sprint 12 P1 — backend-side app state.
//!
//! Most state lives in the React Zustand store; this module
//! tracks what the Rust side needs (active translation handle,
//! cancellation token) for in-flight pipeline runs.

use std::sync::Mutex;

#[derive(Default)]
pub struct AppState {
    pub current_run_id: Mutex<Option<u64>>,
}
