#[cfg(test)]
mod tests {
    use actix_web::body::to_bytes;
    use actix_web::{App, middleware::Logger, test, web};
    use actix_web_httpauth::middleware::HttpAuthentication;
    use diesel::prelude::*;
    use diesel::r2d2::ConnectionManager;
    use dotenv::dotenv;
    use serde_json::Value;
    use uuid::Uuid; // 해당 모듈에서 DB pool 생성 함수가 필요

    use barcode_taste_note::auth;
    use barcode_taste_note::handlers::users_handler::*;

    #[actix_web::test]
    async fn test_users() {
        // setup
        dotenv().ok();
        env_logger::init();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = diesel::r2d2::Pool::builder()
            .build(ConnectionManager::<PgConnection>::new(database_url))
            .expect("Failed to create pool.");
        let app = App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .route("/users", web::get().to(get_users))
            .route("/users/{id}", web::get().to(get_user_by_id))
            .route("/users", web::post().to(add_user))
            .service(
                web::scope("") // 특정 라우트만 인증 적용
                    .wrap(HttpAuthentication::bearer(auth::validator))
                    .route("/users", web::delete().to(delete_user)),
            );
        let test_app = test::init_service(app).await;

        // get_users
        let req = test::TestRequest::get().uri("/users").to_request();
        let res = test::call_service(&test_app, req).await;
        for (key, value) in res.headers() {
            println!("Header: {}: {:?}", key, value);
        }

        let body_bytes = to_bytes(res.into_body()).await.unwrap();
        let json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["result"].as_bool(), Some(true));
        // if let Some(users) = json.get("data").and_then(|d| d.get("users")) {
        //     println!("Users: {}", users);
        // } else {
        //     println!("Couldn't find users key!");
        // }

        // add_user
        // let payload = InputUser {
        //     nick_name: Some("tester".to_string()),
        //     sub: "sub-1234".to_string(),
        // };

        // let req = test::TestRequest::post()
        //     .uri("/users")
        //     .set_json(&payload)
        //     .to_request();
        // let resp = test::call_service(&test_app, req).await;
        // assert!(resp.status().is_success());

        // get_user_by_id
        // let uri = format!("/users/{}", user.id);
        // let req = test::TestRequest::get().uri(&uri).to_request();
        // let resp = test::call_service(&test_app, req).await;
        // assert!(resp.status().is_success());

        // delete_user
    }
}
