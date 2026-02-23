// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "point", schema = "pg_catalog"))]
    pub struct Point;
}

diesel::table! {
    barcodes (id) {
        id -> Uuid,
        barcode_id -> Text,
        product_id -> Uuid,
    }
}

diesel::table! {
    favorites (id) {
        id -> Uuid,
        product_id -> Uuid,
        user_id -> Uuid,
    }
}

diesel::table! {
    flavor_tags (id) {
        id -> Uuid,
        flavor -> Int2,
        product_id -> Uuid,
        note_id -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::Point;

    notes (id) {
        id -> Uuid,
        user_id -> Uuid,
        product_id -> Uuid,
        body -> Nullable<Text>,
        registered -> Timestamptz,
        rating -> Int2,
        public_scope -> Int2,
        details -> Nullable<Jsonb>,
        location -> Nullable<Point>,
    }
}

diesel::table! {
    product_images (id) {
        id -> Uuid,
        product_id -> Nullable<Uuid>,
        note_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
        registered -> Timestamptz,
    }
}

diesel::table! {
    products (id) {
        id -> Uuid,
        name -> Text,
        #[sql_name = "type"]
        type_ -> Int2,
        desc -> Nullable<Text>,
        rating -> Nullable<Float4>,
        flavors -> Nullable<Jsonb>,
        registered -> Timestamptz,
        note_count -> Int4,
    }
}

diesel::table! {
    reports (id) {
        id -> Uuid,
        product_id -> Nullable<Uuid>,
        user_id -> Uuid,
        body -> Nullable<Text>,
        state -> Nullable<Int2>,
        reply -> Text,
        registered -> Nullable<Timestamptz>,
        #[sql_name = "type"]
        type_ -> Int2,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        nick_name -> Text,
        sub -> Text,
        intro -> Nullable<Text>,
        image_id -> Nullable<Uuid>,
    }
}

diesel::joinable!(barcodes -> products (product_id));
diesel::joinable!(favorites -> products (product_id));
diesel::joinable!(flavor_tags -> notes (note_id));
diesel::joinable!(flavor_tags -> products (product_id));
diesel::joinable!(product_images -> notes (note_id));
diesel::joinable!(product_images -> products (product_id));

diesel::allow_tables_to_appear_in_same_query!(
    barcodes,
    favorites,
    flavor_tags,
    notes,
    product_images,
    products,
    reports,
    users,
);
