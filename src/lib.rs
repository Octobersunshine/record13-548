pub mod audio;
pub mod errors;
pub mod library;
pub mod models;
pub mod routes;

use std::sync::Arc;
use library::CopyrightLibrary;

#[derive(Clone)]
pub struct AppState {
    pub library: Arc<CopyrightLibrary>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            library: Arc::new(CopyrightLibrary::new()),
        }
    }

    pub fn with_library(library: CopyrightLibrary) -> Self {
        Self {
            library: Arc::new(library),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
