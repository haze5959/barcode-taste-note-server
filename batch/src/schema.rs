diesel::table! {
    product_images (id) {
        id -> Uuid,
        product_id -> Nullable<Uuid>,
        note_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
        registered -> Timestamp,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use pgvector::sql_types::*;

    products (id) {
        id -> Uuid,
        name -> Text,
        #[sql_name = "type"]
        type_ -> SmallInt,
        desc -> Nullable<Text>,
        rating -> Nullable<Float4>,
        flavor_infos -> Nullable<Jsonb>,
        registered -> Timestamptz,
        note_count -> Int4,
        embedding -> Nullable<Vector>,
        details -> Nullable<Jsonb>,
        is_verified -> Bool,
    }
}

diesel::table! {
    barcodes (id) {
        id -> Uuid,
        barcode_id -> Text,
        product_id -> Uuid,
    }
}

diesel::joinable!(barcodes -> products (product_id));
diesel::joinable!(product_images -> products (product_id));

diesel::allow_tables_to_appear_in_same_query!(
    barcodes,
    product_images,
    products,
);
