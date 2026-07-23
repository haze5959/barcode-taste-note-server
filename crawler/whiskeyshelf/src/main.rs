use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

use diesel::pg::PgConnection;
use diesel::prelude::*;

// ============================================================
// Diesel Database Schema (barcodes Table)
// ============================================================

diesel::table! {
    barcodes (id) {
        id -> Nullable<Uuid>,
        barcode_id -> Text,
        product_id -> Nullable<Uuid>,
    }
}

// ============================================================
// Shopify API Response Models
// ============================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ShopifyProductsResponse {
    products: Vec<ShopifyProduct>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ShopifyProduct {
    id: i64,
    title: String,
    handle: String,
    product_type: Option<String>,
    tags: Option<Vec<String>>,
    variants: Vec<ShopifyVariant>,
    images: Vec<ShopifyImage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ShopifyVariant {
    id: i64,
    sku: Option<String>,
    barcode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ShopifyImage {
    src: String,
}

// ============================================================
// Gemini Request/Response Models
// ============================================================

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidate {
    content: Option<GeminiCandidateContent>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidateContent {
    parts: Option<Vec<GeminiCandidatePart>>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidatePart {
    text: Option<String>,
}

// ============================================================
// Output JSON Model (new_product.json)
// ============================================================

#[derive(Debug, Serialize, Deserialize)]
struct OutputProduct {
    barcode: String,
    product_name: String,
    desc: String,
    #[serde(rename = "type")]
    type_: String,
    image_url: Option<String>,
    details: Option<serde_json::Value>,
}

// ============================================================
// DB Helpers
// ============================================================

fn establish_db_connection() -> Option<PgConnection> {
    let database_url = env::var("DATABASE_URL").ok()?;
    match PgConnection::establish(&database_url) {
        Ok(conn) => {
            println!("[DB] Connected to PostgreSQL successfully.");
            Some(conn)
        }
        Err(e) => {
            eprintln!("[DB Warning] Failed to connect to DB: {}. Proceeding without DB duplicate checks.", e);
            None
        }
    }
}

fn check_barcode_exists(conn: &mut PgConnection, code: &str) -> bool {
    use diesel::dsl::{exists, select};
    select(exists(barcodes::table.filter(barcodes::barcode_id.eq(code))))
        .get_result(conn)
        .unwrap_or(false)
}

// ============================================================
// Category Mapping
// ============================================================

fn map_category(product_type: Option<&str>, title: &str) -> &'static str {
    let ptype = product_type.unwrap_or("");
    let combined = format!("{} {}", ptype, title).to_lowercase();

    if combined.contains("whisky")
        || combined.contains("whiskey")
        || combined.contains("bourbon")
        || combined.contains("rye")
        || combined.contains("scotch")
    {
        "whisky"
    } else if combined.contains("wine") || combined.contains("champagne") {
        "wine"
    } else if combined.contains("beer") || combined.contains("ale") || combined.contains("lager") {
        "beer"
    } else if combined.contains("soju") || combined.contains("sake") {
        "sake"
    } else if combined.contains("liqueur") {
        "liqueur"
    } else if combined.contains("tequila")
        || combined.contains("mezcal")
        || combined.contains("rum")
        || combined.contains("vodka")
        || combined.contains("gin")
        || combined.contains("brandy")
        || combined.contains("cognac")
        || combined.contains("spirit")
    {
        "spirit"
    } else {
        "whisky"
    }
}

// ============================================================
// Gemini API Call
// ============================================================

const GEMINI_PROMPT_TEMPLATE: &str = r#"Analyze F&B product name. If NOT F&B, return: {"error":"Not an F&B product"}.
Name: Core English name ONLY in Title Case. ALWAYS KEEP the Brand/Distillery/Producer name at the beginning (e.g., "10th Mountain Spirits", "10th Mountain", "Buffalo Trace"). NEVER remove the brand name. KEEP liquor types and expressions (e.g., "Bourbon Whiskey", "Rye Whiskey", "Straight Bourbon", "Single Malt", "Blended Scotch"). EXCLUDE ONLY promo text, limited/seasonal tags, volume/capacity (e.g. 750mL, 1L), container types (Can, Bottle, Box), and hyphens. KEEP aging as "X Years Old" (e.g., 7YO → "7 Years Old"). EXCLUDE vintage year from product name for wine (e.g., exclude 2019, 2020).
Desc: Professional factual English desc (<200 chars). No repeating name. Include production methods, flavor markers, market specs.
Return JSON: {"product_name":"...","desc":"...","details":{"style":<int>,"manufacturer":"<str>","country":"<2-letter_iso>","alcohol":<float>,"grape":<int>,"ibu":<int>}}
Rules for 'details': 'grape' ONLY if wine. 'ibu' ONLY if beer. Use null for any field you are not confident about.
STYLE: Wine(0:red,1:white,2:rose,3:sparkling,4:dessert,5:fortified,6:natural),Whisky(100:singleMalt,101:blended,102:singleGrain,103:bourbon,104:rye,105:tennessee,106:irish,107:japanese,108:canadian,109:other),Beer(200:lager,201:pilsner,202:paleAle,203:ipa,204:hazyIpa,205:stout,206:porter,207:wheat,208:sour,209:belgianAle,210:amber),Asian(300:soju,301:fruitSoju,302:junmai,303:junmaiGinjo,304:junmaiDaiginjo,305:ginjo,306:daiginjo,307:honjozo,308:nigori,309:cheongju,310:yakju,311:makgeolli),Spirits(400:vodka,401:gin,402:lightRum,403:darkRum,404:spicedRum,405:tequila,406:mezcal,407:brandy,408:cognac,409:armagnac,410:absinthe,411:baijiu,412:liqueur),Cocktail(500:classic,501:craft,502:tiki,503:sour,504:highball,505:frozen,506:mocktail),Coffee(600:espresso,601:americano,602:latte,603:cappuccino,604:macchiato,605:flatWhite,606:mocha,607:drip,608:pourOver,609:coldBrew,610:singleOrigin),Other(700:other)
GRAPE(Wine ONLY): Red(0:cabSauv,1:merlot,2:pinotNoir,3:syrah,4:malbec,5:sangiovese,6:tempranillo,7:nebbiolo,8:grenache,9:zinfandel,10:cabFranc,11:carmenere,12:gamay,13:montepulciano,14:petitVerdot),White(100:chardonnay,101:sauvBlanc,102:riesling,103:pinotGrigio,104:gewurztraminer,105:cheninBlanc,106:viognier,107:semillon,108:moscato,109:albarino,110:pinotBlanc),Other(200:redBlend,201:whiteBlend,299:other)
Product name: "{}" "#;

async fn call_gemini(
    client: &reqwest::Client,
    api_key: &str,
    product_name: &str,
) -> Result<(String, String, Option<serde_json::Value>), String> {
    let prompt = GEMINI_PROMPT_TEMPLATE.replace("{}", product_name);

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart { text: prompt }],
        }],
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let res = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Gemini API error: {}", err_text));
    }

    let gemini_resp: GeminiResponse = res
        .json()
        .await
        .map_err(|e| format!("Parsing failed: {}", e))?;

    let text_output = gemini_resp
        .candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content)
        .and_then(|mut content| content.parts.take())
        .and_then(|mut parts| parts.pop())
        .and_then(|p| p.text)
        .ok_or_else(|| "Empty Gemini response".to_string())?;

    let cleaned = text_output
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if cleaned.contains("\"error\"") {
        return Err(format!("Gemini returned error for: {}", product_name));
    }

    let parsed: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("JSON parse failed: {} | raw: {}", e, cleaned))?;

    let name = parsed["product_name"]
        .as_str()
        .ok_or_else(|| "Missing product_name field".to_string())?
        .to_string();
    let desc = parsed["desc"]
        .as_str()
        .ok_or_else(|| "Missing desc field".to_string())?
        .to_string();
    let details = parsed.get("details").cloned();

    Ok((name, desc, details))
}

// ============================================================
// MAIN
// ============================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let mut db_conn = establish_db_connection();

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .build()?;

    let base_url = "https://whiskeyshelf.com/collections/all/products.json";

    let mut all_results: Vec<OutputProduct> = Vec::new();
    let mut processed_barcodes: HashSet<String> = HashSet::new();
    let mut page_num: u32 = 1;
    let mut consecutive_empty = 0;

    println!(">>> Starting Whiskeyshelf Crawler...");

    loop {
        let url = format!("{}?tab=products&page={}", base_url, page_num);
        println!("\n[Page {}] Fetching: {}", page_num, url);

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[Page {}] Request error: {}", page_num, e);
                consecutive_empty += 1;
                if consecutive_empty >= 3 {
                    println!("3 consecutive errors. Stopping.");
                    break;
                }
                sleep(Duration::from_secs(2)).await;
                page_num += 1;
                continue;
            }
        };

        if !resp.status().is_success() {
            eprintln!("[Page {}] HTTP error: {}", page_num, resp.status());
            consecutive_empty += 1;
            if consecutive_empty >= 3 {
                break;
            }
            sleep(Duration::from_secs(2)).await;
            page_num += 1;
            continue;
        }

        let shopify_resp: ShopifyProductsResponse = match resp.json().await {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!("[Page {}] Failed to parse JSON: {}", page_num, e);
                consecutive_empty += 1;
                if consecutive_empty >= 3 {
                    break;
                }
                page_num += 1;
                continue;
            }
        };

        if shopify_resp.products.is_empty() {
            println!("[Page {}] Empty page → Crawling finished.", page_num);
            consecutive_empty += 1;
            if consecutive_empty >= 2 {
                break;
            }
            page_num += 1;
            continue;
        }

        consecutive_empty = 0;
        println!("[Page {}] Processing {} products...", page_num, shopify_resp.products.len());

        for prod in shopify_resp.products {
            let raw_title = prod.title.trim().to_string();
            let image_url = prod.images.first().map(|i| i.src.clone());
            let type_str = map_category(prod.product_type.as_deref(), &raw_title).to_string();

            for variant in prod.variants {
                let raw_code = match variant.barcode.as_deref().or(variant.sku.as_deref()) {
                    Some(c) if !c.trim().is_empty() => c.trim(),
                    _ => continue,
                };

                let clean_barcode = raw_code.replace('-', "").replace(' ', "");
                if clean_barcode.is_empty() || clean_barcode.len() < 5 {
                    continue;
                }

                // Check in-memory set
                if processed_barcodes.contains(&clean_barcode) {
                    println!("  → [In-Memory Skip] Barcode {} already processed", clean_barcode);
                    continue;
                }

                // Check DB
                if let Some(ref mut conn) = db_conn {
                    if check_barcode_exists(conn, &clean_barcode) {
                        println!("  → [DB Skip] Barcode {} already exists in DB", clean_barcode);
                        processed_barcodes.insert(clean_barcode);
                        continue;
                    }
                }

                println!("  + Processing product: '{}' (Barcode: {})", raw_title, clean_barcode);

                match call_gemini(&client, &api_key, &raw_title).await {
                    Ok((product_name, desc, details)) => {
                        println!("    ✓ Cleaned Name: {}", product_name);
                        all_results.push(OutputProduct {
                            barcode: clean_barcode.clone(),
                            product_name,
                            desc,
                            type_: type_str.clone(),
                            image_url: image_url.clone(),
                            details,
                        });
                        processed_barcodes.insert(clean_barcode);
                    }
                    Err(e) => {
                        println!("    ✗ Gemini Error: {}", e);
                    }
                }

                sleep(Duration::from_millis(500)).await;
            }
        }

        save_output(&all_results);
        page_num += 1;
        sleep(Duration::from_secs(1)).await;
    }

    println!("\nComplete! Total {} products saved to new_product.json.", all_results.len());
    save_output(&all_results);

    Ok(())
}

fn save_output(products: &[OutputProduct]) {
    let json_str = match serde_json::to_string_pretty(products) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("JSON serialization failed: {}", e);
            return;
        }
    };

    if let Err(e) = std::fs::write("output/new_product.json", &json_str) {
        let _ = std::fs::create_dir_all("output");
        if let Err(e2) = std::fs::write("output/new_product.json", &json_str) {
            eprintln!("File write failed: {} / {}", e, e2);
        }
    }
}
