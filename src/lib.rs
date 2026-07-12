pub mod streaming {
    pub mod range;
    pub mod direct;
}
pub mod error;
pub mod config;
pub mod api;
pub fn server_name() -> &'static str { "medienserver" }
