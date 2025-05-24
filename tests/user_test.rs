#[cfg(test)]
mod tests {
    use actix_web::http::header;
    use actix_web::{App, HttpMessage, middleware::Logger, test, web};
    use actix_web_httpauth::middleware::HttpAuthentication;
    use barcode_taste_note::models::{CommonResponse, User};
    use barcode_taste_note::errors::CommonResponseError;
    use diesel::prelude::*;
    use diesel::r2d2::ConnectionManager;
    use dotenv::dotenv;

    use barcode_taste_note::auth;
    use barcode_taste_note::handlers::users_handler::*;
    use barcode_taste_note::utils::logger::*;
    use barcode_taste_note::utils::response_mapper::*;

    #[actix_web::test]
    async fn test_users() {
        let auth_token = "Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6InZycFo3Z0I3VWlqQVpCVTVQUVl2byJ9.eyJpc3MiOiJodHRwczovL2Rldi0wZXkyemttZTJ6ZjZsY3p1LnVzLmF1dGgwLmNvbS8iLCJzdWIiOiI2aXhZTW56UXdDSGd6dXdxWEVLdmZNVHhHYVpmSHR5WEBjbGllbnRzIiwiYXVkIjoiaHR0cHM6Ly9kZXYtMGV5MnprbWUyemY2bGN6dS51cy5hdXRoMC5jb20vYXBpL3YyLyIsImlhdCI6MTc0ODA3NDQwNSwiZXhwIjoxNzQ4MTYwODA1LCJndHkiOiJjbGllbnQtY3JlZGVudGlhbHMiLCJhenAiOiI2aXhZTW56UXdDSGd6dXdxWEVLdmZNVHhHYVpmSHR5WCJ9.Lnq0f8mOmP5pnA9zm-kPo0LHF7Rh_ffg13u8szPMysTekIh-JAH2erkLNEFvH5ON3y-X0EQSiYzi3bRKluPb2Ld8YHj5APIxW_O18V2_q3xzWiJW4KUMQDVdvK20CgIOYMTYf0oPYR3vNIGu4t350NyeV7-Ju5gkmAzhzNmynYTl91pGhcUnCj5Opp6UvlrYO6sVe89xASGNo6TKj4cyV9sWfikf9etI6fBueYA8v6JLQ656qrb6k1en0KQUFa4QlI8jYPQtFJZjJaSZksHg7VxpkH84Dw1cMwdj5QnP-3G66jF9RwzK-_WpO_fvAGDEvMP7232STCSEE5G4jZbbZg";
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
            .route("/users/id/{id}", web::get().to(get_user_by_id))
            .service(
                web::scope("") // 특정 라우트만 인증 적용
                    .wrap(HttpAuthentication::bearer(auth::validator))
                    .route("/users", web::post().to(add_user))
                    .route("/users/me", web::get().to(get_my_info))
                    .route("/users/me", web::put().to(update_user_nick))
                    .route("/users/me", web::delete().to(delete_user)),
            );
        let test_app = test::init_service(app).await;

        println!("[start]---- {} ----", "GET /users");
        let req = test::TestRequest::get().uri("/users").to_request();
        let res = test::call_service(&test_app, req).await;
        print_response_log(res).await;
        println!("[finish]---- {} ----\n", "GET /users");

        println!("[start]---- {} ----", "GET /users/me");
        let req = test::TestRequest::get()
            .uri("/users/me")
            .insert_header((header::AUTHORIZATION, auth_token))
            .to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<Option<User>> = response_to_model(res).await;
        print_response_model(&res_model);
        println!("[finish]---- {} ----\n", "GET /users/me");
        
        let mut me: User;
        if res_model.result {
            me = res_model.data.unwrap();
        } else {
            if res_model.error == Some(CommonResponseError::RecordNotFound as u8) {
                // 계정이 없다면 계정을 새로 만든다
                println!("[start]---- {} ----", "POST /users");
                let payload = AddUserParams {
                    nick_name: Some(name01.to_string()),
                };

                let req = test::TestRequest::post()
                    .uri("/users")
                    .insert_header((header::AUTHORIZATION, auth_token))
                    .set_json(&payload)
                    .to_request();
                print_header_log(req.headers());
                let res = test::call_service(&test_app, req).await;
                let res_model: CommonResponse<Option<User>> = response_to_model(res).await;
                print_response_model(&res_model);
                assert!(res_model.result);
                me = res_model.data.unwrap();
                println!("[finish]---- {} ----\n", "POST /users");
            } else {
                panic!("response fail: {:?}", res_model);
            }
        }
        println!("[me]: {:?}", me);

        println!("[start]---- {} ----", "PUT /users/me");
        let payload = AddUserParams {
            nick_name: Some(name02.to_string()),
        };

        let req = test::TestRequest::put()
            .uri("/users/me")
            .insert_header((header::AUTHORIZATION, auth_token))
            .set_json(&payload)
            .to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<Option<User>> = response_to_model(res).await;
        print_response_model(&res_model);
        assert!(res_model.result);
        println!("[finish]---- {} ----\n", "PUT /users/me");

        println!("[start]---- {} ----", "GET /users/id/:id");
        let uri = format!("/users/id/{}", res_model.data.unwrap().id);
        let req = test::TestRequest::get().uri(&uri).to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<Option<User>> = response_to_model(res).await;
        print_response_model(&res_model);
        me = res_model.data.unwrap();
        assert!(me.nick_name == name02);
        println!("[finish]---- {} ----\n", "GET /users/id/:id");

        println!("[start]---- {} ----", "DELETE /users/me");
        let req = test::TestRequest::delete()
            .uri("/users/me")
            .insert_header((header::AUTHORIZATION, auth_token))
            .to_request();
        let res = test::call_service(&test_app, req).await;
        let res_model: CommonResponse<Option<bool>> = response_to_model(res).await;
        print_response_model(&res_model);
        assert!(res_model.result && res_model.data.unwrap());
        println!("[finish]---- {} ----\n", "DELETE /users/me");
    }
}
