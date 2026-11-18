use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct IslandState {
    pub expanded: bool,
    pub mode: String,
    pub title: String,
}

static EXPANDED: Mutex<bool> = Mutex::new(false);

pub fn current_state() -> IslandState {
    let expanded = *EXPANDED.lock().unwrap_or_else(|e| e.into_inner());
    IslandState {
        expanded,
        mode: if expanded {
             "expanded".into() 
        } else { 
             "compact".into() 
        },
        title: "Nexora Island".into(),
    }
}

pub fn set_expanded(expanded: bool) -> IslandState {
    if let Ok(mut guard) = EXPANDED.lock() {
        *guard = expanded;
    }
    current_state()
}
