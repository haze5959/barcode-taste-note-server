use actix_web::{App, HttpServer, middleware::Logger, web};
use actix_web_httpauth::middleware::HttpAuthentication;
use dotenv::dotenv;

use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;

use barcode_taste_note::handlers;
use barcode_taste_note::auth;

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
            .route("/products", web::post().to(handlers::products_handlers::create_product))
            .route("/products", web::get().to(handlers::products_handlers::get_products_list))
            .route("/products/{id}", web::get().to(handlers::products_handlers::get_product_by_id))
            .route("/products/barcode/{barcode_id}", web::get().to(handlers::products_handlers::get_product_by_barcode))
            .service(
                web::scope("") // 특정 라우트만 인증 적용
                    .wrap(HttpAuthentication::bearer(auth::validator))
                    .route("/users", web::post().to(handlers::users_handler::add_user))
                    .route("/users/me", web::get().to(handlers::users_handler::get_my_info))
                    .route("/users/me", web::put().to(handlers::users_handler::update_user_nick))
                    .route(
                        "/users/me",
                        web::delete().to(handlers::users_handler::delete_user),
                    ),
            )
    })
    .bind("172.30.1.21:5959")?
    .run()
    .await
}
