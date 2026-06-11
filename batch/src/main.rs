use std::env;

mod db;
mod models;
mod schema;
mod r2;
mod cohere;
mod jobs;

use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: cargo run <command>");
        eprintln!("Available commands: clean_image, add_product_with_json, backup_db, backup_image, reembed_products");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "clean_image" => {
            println!("Starting 'clean_image' batch job...");
            jobs::clean_image::run().await;
            println!("Batch job 'clean_image' completed.");
        }
        "add_product_with_json" => {
            println!("Starting 'add_product_with_json' batch job...");
            let mut conn = db::establish_connection();
            let r2 = r2::R2Client::new().await;
            jobs::add_product_with_json::run(&mut conn, &r2).await;
            println!("Batch job 'add_product_with_json' completed.");
        }
        "backup_db" => {
            println!("Starting 'backup_db' batch job...");
            jobs::backup_db::run().await;
            println!("Batch job 'backup_db' completed.");
        }
        "backup_image" => {
            println!("Starting 'backup_image' batch job...");
            jobs::backup_image::run().await;
            println!("Batch job 'backup_image' completed.");
        }
        "reembed_products" => {
            println!("Starting 'reembed_products' batch job...");
            let mut conn = db::establish_connection();
            jobs::reembed_products::run(&mut conn).await;
            println!("Batch job 'reembed_products' completed.");
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}
