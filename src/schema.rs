// @generated automatically by Diesel CLI.

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    barcodes (id) {
        id -> Uuid,
        barcode_id -> Text,
        product_id -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    favorites (id) {
        id -> Uuid,
        product_id -> Uuid,
        user_id -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    flavor_tags (id) {
        id -> Uuid,
        flavor -> Int2,
        product_id -> Uuid,
        note_id -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    follows (id) {
        user_id -> Uuid,
        following_user_id -> Uuid,
        id -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    notes (id) {
        id -> Uuid,
        user_id -> Uuid,
        product_id -> Uuid,
        body -> Nullable<Text>,
        registered -> Timestamptz,
        rating -> Int2,
        public_scope -> Int2,
        details -> Nullable<Jsonb>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    product_images (id) {
        id -> Uuid,
        product_id -> Nullable<Uuid>,
        note_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
        registered -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    products (id) {
        id -> Uuid,
        name -> Text,
        #[sql_name = "type"]
        type_ -> Int2,
        desc -> Nullable<Text>,
        rating -> Nullable<Float4>,
        flavor_infos -> Nullable<Jsonb>,
        registered -> Timestamptz,
        note_count -> Int4,
        embedding -> Nullable<Vector>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    reports (id) {
        id -> Uuid,
        product_id -> Nullable<Uuid>,
        user_id -> Uuid,
        body -> Nullable<Text>,
        state -> Nullable<Int2>,
        reply -> Nullable<Text>,
        registered -> Nullable<Timestamptz>,
        #[sql_name = "type"]
        type_ -> Int2,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::Vector;

    users (id) {
        id -> Uuid,
        nick_name -> Text,
        sub -> Text,
        intro -> Nullable<Text>,
        image_id -> Nullable<Uuid>,
        registered -> Nullable<Timestamptz>,
        premium_expire_at -> Nullable<Timestamptz>,
    }
}

diesel::joinable!(barcodes -> products (product_id));
diesel::joinable!(favorites -> products (product_id));
diesel::joinable!(favorites -> users (user_id));
diesel::joinable!(flavor_tags -> notes (note_id));
diesel::joinable!(flavor_tags -> products (product_id));
diesel::joinable!(notes -> products (product_id));
diesel::joinable!(notes -> users (user_id));
diesel::joinable!(product_images -> notes (note_id));
diesel::joinable!(product_images -> products (product_id));
diesel::joinable!(reports -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    barcodes,
    favorites,
    flavor_tags,
    follows,
    notes,
    product_images,
    products,
    reports,
    users,
);
