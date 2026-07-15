pub mod streaming {
    pub mod range;
    pub mod direct;
}
pub mod scanner {
    pub mod scene;
    pub mod nfo;
}
pub mod error;
pub mod db;
pub mod config;
pub mod auth;
pub mod api;
pub fn server_name() -> &'static str { "medienserver" }
