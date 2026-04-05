use actix_web::{App, HttpServer, middleware::Logger, web, http::header};
use actix_web_httpauth::middleware::HttpAuthentication;
use actix_cors::Cors;
use dotenv::dotenv;
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;

use barcode_taste_note::handlers;
use barcode_taste_note::auth;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok(); // Reads the .env file
    env_logger::init();

    // Initialize rustls CryptoProvider (required for rustls 0.23+ when multiple/no default providers are present)
    let _ = rustls::crypto::ring::default_provider().install_default();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = diesel::r2d2::Pool::builder()
        // .max_size(POOL_CONNECTION_SIZE)
        .build(ConnectionManager::<PgConnection>::new(database_url))
        .expect("Failed to create pool.");

    let r2_client = barcode_taste_note::utils::r2::R2Client::new().await;
    let r2_data = web::Data::new(r2_client);

    // load TLS keys
    // to create a self-signed temporary cert for testing:
    // `openssl req -x509 -newkey rsa:4096 -nodes -keyout key.pem -out cert.pem -days 365 -subj '/CN=localhost'`
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file("ssl/server.key", SslFiletype::PEM)
        .expect("Failed to load private key");
    builder
        .set_certificate_chain_file("ssl/server.crt")
        .expect("Failed to load certificate");

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .wrap(
                Cors::default()
                    .allowed_origin("https://barnote.net")
                    .allowed_origin_fn(|origin, _req_head| {
                        origin.as_bytes().ends_with(b".barnote.net")
                    })
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE"])
                    .allowed_headers(vec![
                        header::AUTHORIZATION,
                        header::ACCEPT,
                        header::CONTENT_TYPE,
                    ])
                    .max_age(3600),
            )
            .app_data(web::Data::new(pool.clone()))
            .app_data(r2_data.clone())
            // Public routes
            // Users
            .route("/users", web::get().to(handlers::users_handler::get_users))
            .route("/users/{id}", web::get().to(handlers::users_handler::get_user_by_id))
            // Products
            .route("/products", web::post().to(handlers::products_handlers::create_product))
            .route("/products", web::get().to(handlers::products_handlers::get_products_list))
            .route("/products/favorite", web::get().to(handlers::products_handlers::get_favorite_products_list_by_user_id))
            .route("/products/barcode/{barcode_id}", web::get().to(handlers::products_handlers::get_product_by_barcode))
            .route("/products/{id}", web::get().to(handlers::products_handlers::get_product_by_id))
            // Notes
            .route("/notes", web::get().to(handlers::notes_handlers::get_notes_list))
            .route("/notes/user/{id}", web::get().to(handlers::notes_handlers::get_notes_by_user))
            .route("/notes/{id}", web::get().to(handlers::notes_handlers::get_note_by_id))
            // Images
            .route("/images", web::get().to(handlers::images_handlers::get_images))
            // BTN APP
            .route("/btn/home", web::get().to(handlers::btn_app_handlers::get_home_info))
            // Webhooks
            .route("/webhooks/appstore", web::post().to(handlers::webhook_handlers::handle_appstore_notification))
            // Authenticated routes
            .service(
                web::scope("api")
                            .wrap(HttpAuthentication::bearer(auth::validator))
                            // Users
                            .route("/users", web::post().to(handlers::users_handler::add_user))
                            .route("/users/me", web::get().to(handlers::users_handler::get_my_info))
                            .route("/users/favorites", web::get().to(handlers::users_handler::get_my_favorites))
                            .route("/users/me", web::put().to(handlers::users_handler::update_user_nick))
                            .route("/users/me", web::delete().to(handlers::users_handler::delete_user))
                            .route("/users/search", web::get().to(handlers::users_handler::search_users))
                            .route("/users/follower", web::get().to(handlers::users_handler::get_followers))
                            .route("/users/following", web::get().to(handlers::users_handler::get_followings))
                            .route("/users/following", web::post().to(handlers::users_handler::follow_user))
                            .route("/users/following/{id}", web::delete().to(handlers::users_handler::unfollow_user))
                            .route("/users/{id}", web::get().to(handlers::users_handler::get_user_by_id_with_auth))
                            // Products
                            .route("/products/favorite", web::get().to(handlers::products_handlers::get_favorite_products_list))
                            .route("/products/favorite", web::post().to(handlers::products_handlers::set_product_favorite))
                            .route("/products/tasted", web::get().to(handlers::products_handlers::get_tasted_products_list))
                            .route("/products/ai", web::post().to(handlers::products_handlers::create_product_by_ai))
                            .route("/products/barcode/{barcode_id}", web::get().to(handlers::products_handlers::get_product_by_barcode_with_auth))
                            .route("/products/{id}", web::get().to(handlers::products_handlers::get_product_by_id_with_auth))
                            // Notes
                            .route("/notes/calendar", web::get().to(handlers::notes_handlers::get_notes_calendar))
                            .route("/notes/rating", web::get().to(handlers::notes_handlers::get_notes_by_rating))
                            .route("/notes", web::get().to(handlers::notes_handlers::get_api_notes_list))
                            .route("/notes", web::post().to(handlers::notes_handlers::create_note))
                            .route("/notes/{id}", web::put().to(handlers::notes_handlers::update_note))
                            .route("/notes/{id}", web::delete().to(handlers::notes_handlers::delete_note))
                            // Images
                            .route("/images/profile", web::post().to(handlers::images_handlers::upload_profile_image))
                            .route("/images", web::post().to(handlers::images_handlers::upload_image))
                            .route("/images/{id}", web::delete().to(handlers::images_handlers::delete_image))
                            // BTN APP
                            .route("/btn/report", web::get().to(handlers::btn_app_handlers::get_my_reports))
                            .route("/btn/report", web::post().to(handlers::btn_app_handlers::create_report))
                    )
            .service(
                web::scope("admin")
                    .service(
                        web::scope("")
                            .wrap(HttpAuthentication::bearer(auth::validator))
                            .route("/dashboard", web::get().to(handlers::admin_handlers::get_dashboard)) // New Dashboard Endpoint added correctly here too!
                            .route("/notes", web::get().to(handlers::admin_handlers::get_admin_notes))
                            .route("/report", web::get().to(handlers::admin_handlers::get_reports))
                            .route("/report", web::put().to(handlers::admin_handlers::update_admin_report))
                            .route("/product/barcodes", web::get().to(handlers::admin_handlers::get_admin_product_barcodes))
                            .route("/product/main_image", web::get().to(handlers::admin_handlers::get_admin_product_main_image))
                            .route("/product", web::put().to(handlers::admin_handlers::update_admin_product))
                            .route("/product/merge", web::post().to(handlers::admin_handlers::merge_admin_product))
                            .route("/products/{product_id}", web::delete().to(handlers::admin_handlers::delete_admin_product))
                            .route("/image", web::post().to(handlers::admin_handlers::upload_admin_image))
                            .route("/image/url", web::post().to(handlers::admin_handlers::upload_admin_image_by_url))
                    )
            )
    })
    .bind_openssl("172.30.1.21:5959", builder)?
    .run()
    .await
}
