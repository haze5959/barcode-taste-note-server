pub mod auth;
pub mod errors;
pub mod handlers;
pub mod models;
pub mod schema;
pub mod constants;
pub mod utils;

use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;
#[macro_use]
extern crate diesel;