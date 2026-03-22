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
        type_ -> SmallInt,
        desc -> Nullable<Text>,
        rating -> Nullable<Float4>,
        flavor_infos -> Nullable<Jsonb>,
        registered -> Timestamptz,
        note_count -> Int4,
        embedding -> Nullable<Vector>,
    }
}

diesel::table! {
    barcodes (id) {
        id -> Uuid,
        barcode_id -> Text,
        product_id -> Uuid,
    }
}
