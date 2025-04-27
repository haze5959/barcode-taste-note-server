use actix_web::{App, HttpServer, middleware::Logger, web};
use actix_web_httpauth::middleware::HttpAuthentication;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
use dotenv::dotenv;

mod auth;
mod errors;
mod handlers;
mod models;
mod schema;
mod constants;

pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;

#[macro_use]
extern crate diesel;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok(); // Reads the .env file
    env_logger::init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = diesel::r2d2::Pool::builder()
        // .max_size(POOL_CONNECTION_SIZE)
        .build(ConnectionManager::<PgConnection>::new(database_url))
        .expect("Failed to create pool.");

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .route("/users", web::get().to(handlers::users_handler::get_users))
            .route(
                "/users/{id}",
                web::get().to(handlers::users_handler::get_user_by_id),
            )
            .route("/users", web::post().to(handlers::users_handler::add_user))
            .service(
                web::scope("") // 특정 라우트만 인증 적용
                    .wrap(HttpAuthentication::bearer(auth::validator))
                    
                    .route(
                        "/users",
                        web::delete().to(handlers::users_handler::delete_user),
                    ),
            )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
