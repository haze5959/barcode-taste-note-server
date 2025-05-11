#[cfg(test)]
mod tests {
    use actix_web::http::header;
    use actix_web::{App, HttpMessage, middleware::Logger, test, web};
    use actix_web_httpauth::middleware::HttpAuthentication;
    use barcode_taste_note::models::{CommonResponse, User};
    use diesel::prelude::*;
    use diesel::r2d2::ConnectionManager;
    use dotenv::dotenv;

    use barcode_taste_note::auth;
    use barcode_taste_note::handlers::users_handler::*;
    use barcode_taste_note::utils::logger::*;
    use barcode_taste_note::utils::response_mapper::*;

    #[actix_web::test]
    async fn test_users() {
        let auth_token = "token";
        let name01 = "test_01";
        let name02 = "test_02";

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
            .service(
                web::scope("") // 특정 라우트만 인증 적용
                    .wrap(HttpAuthentication::bearer(auth::validator))
                    .route("/users", web::post().to(add_user))
                    .route("/users/me", web::get().to(get_my_info))
                    .route("/users/me", web::put().to(update_user_nick))
                    .route("/users/me", web::delete().to(delete_user)),
            );
        let test_app = test::init_service(app).await;

        // GET /users
        let req = test::TestRequest::get().uri("/users").to_request();
        print_header_log(req.headers());
        let res = test::call_service(&test_app, req).await;
        print_response_log(res).await;

        // GET /users/me
        let req = test::TestRequest::get()
            .uri("/users/me")
            .insert_header((header::AUTHORIZATION, auth_token))
            .to_request();
        print_header_log(req.headers());
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<User> = response_to_model(res).await;
        let mut me: User;
        if res_model.result {
            me = res_model.data;
        } else {
            if res_model.error == Some(1) {
                // 계정이 없다면 계정을 새로 만든다
                // POST /users
                let payload = AddUserParams {
                    nick_name: Some(name01.to_string()),
                };

                let req = test::TestRequest::post()
                    .uri("/users")
                    .insert_header((header::AUTHORIZATION, auth_token))
                    .set_json(&payload)
                    .to_request();
                let res = test::call_service(&test_app, req).await;
                let res_model: CommonResponse<User> = response_to_model(res).await;
                assert!(res_model.result);
                me = res_model.data;
            } else {
                panic!("response fail: {:?}", res_model);
            }
        }

        print_model(&me);

        // PUT /users/me
        let payload = AddUserParams {
            nick_name: Some(name02.to_string()),
        };

        let req = test::TestRequest::put()
            .uri("/users")
            .insert_header((header::AUTHORIZATION, auth_token))
            .set_json(&payload)
            .to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<User> = response_to_model(res).await;
        assert!(res_model.result);

        // GET /users/:id
        let uri = format!("/users/{}", res_model.data.id);
        let req = test::TestRequest::get().uri(&uri).to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<User> = response_to_model(res).await;
        me = res_model.data;
        assert!(me.nick_name == name02);

        // DELETE /users/me
        let req = test::TestRequest::delete()
            .uri("/users/me")
            .insert_header((header::AUTHORIZATION, auth_token))
            .to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<bool> = response_to_model(res).await;
        assert!(res_model.result && res_model.data);
    }
}
