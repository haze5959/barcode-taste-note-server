// @generated automatically by Diesel CLI.

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
        barcode_id -> Uuid,
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
    notes (id) {
        id -> Uuid,
        user_id -> Uuid,
        barcode_id -> Uuid,
        body -> Nullable<Text>,
        registerd -> Date,
        rating -> Int2,
        public_scope -> Int2,
    }
}

diesel::table! {
    product_images (id) {
        id -> Uuid,
        barcode_id -> Uuid,
        note_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        nick_name -> Text,
        sub -> Text,
    }
}

diesel::joinable!(barcodes -> products (product_id));
diesel::joinable!(favorites -> products (barcode_id));
diesel::joinable!(flavor_tags -> notes (note_id));
diesel::joinable!(flavor_tags -> products (product_id));
diesel::joinable!(product_images -> notes (note_id));
diesel::joinable!(product_images -> products (barcode_id));

diesel::allow_tables_to_appear_in_same_query!(
    barcodes,
    favorites,
    flavor_tags,
    notes,
    product_images,
    products,
    users,
);
