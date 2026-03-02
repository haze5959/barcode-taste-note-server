use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct ScrapedProduct {
    pub name: String,
    pub type_: i16,
    pub desc: Option<String>,
    pub image_url: Option<String>,
}

fn parse_category(tags_str: &str) -> i16 {
    let lower = tags_str.to_lowercase();
    if lower.contains("whisky") || lower.contains("whiskies") || lower.contains("whiskey") { return 0; }
    if lower.contains("wine") || lower.contains("wines") { return 1; }
    if lower.contains("beer") || lower.contains("beers") { return 2; }
    if lower.contains("soju") || lower.contains("sake") { return 3; }
    if lower.contains("liqueur") || lower.contains("liqueurs") || lower.contains("spirit") || lower.contains("spirits") { return 4; }
    if lower.contains("cocktail") || lower.contains("cocktails") { return 5; }
    if lower.contains("coffee") || lower.contains("coffees") { return 6; }
    if lower.contains("beverage") || lower.contains("beverages") { return 7; }
    8
}

fn clean_product_name(name: &str) -> String {
    let mut cleaned = name.replace("&quot;", "\"").replace("&amp;", "&");
    
    static RE_PARENS: OnceLock<Regex> = OnceLock::new();
    let re_parens = RE_PARENS.get_or_init(|| Regex::new(r"\(.*?\)").unwrap());
    cleaned = re_parens.replace_all(&cleaned, " ").to_string();
    
    static RE_ABV: OnceLock<Regex> = OnceLock::new();
    let re_abv = RE_ABV.get_or_init(|| Regex::new(r"(?i)\d+(\.\d+)?\s*%\s*(vol\.?)?").unwrap());
    cleaned = re_abv.replace_all(&cleaned, " ").to_string();
    
    static RE_VOL: OnceLock<Regex> = OnceLock::new();
    let re_vol = RE_VOL.get_or_init(|| Regex::new(r"(?i)\b\d+(\.\d+)?\s*(ml|cl|l|liter|liters|litre|litres)\b").unwrap());
    cleaned = re_vol.replace_all(&cleaned, " ").to_string();
    
    static RE_SPAM: OnceLock<Regex> = OnceLock::new();
    let re_spam = RE_SPAM.get_or_init(|| Regex::new(r"(?i)\b(empty|can only|no drink|used)\b").unwrap());
    cleaned = re_spam.replace_all(&cleaned, " ").to_string();
    
    static RE_SPACES: OnceLock<Regex> = OnceLock::new();
    let re_spaces = RE_SPACES.get_or_init(|| Regex::new(r"\s{2,}").unwrap());
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();
    
    // Remove trailing commas, hyphens or spaces
    cleaned.trim().trim_end_matches(&[',', '-', ' '][..]).trim().to_string()
}


pub async fn scrape_barcode_lookup(barcode: &str) -> Option<ScrapedProduct> {
    let url = format!("https://www.barcodelookup.com/{}", barcode);
    println!(">>> [Scraper] Starting: barcode={}, URL={}", barcode, url);
    
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static("PostmanRuntime/7.51.1"));
    headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static("*/*"));
    headers.insert("Postman-Token", reqwest::header::HeaderValue::from_static("951af5ca-c9fb-42e2-b112-347500221137"));
    
    // Exact Cookie from Postman
    let cookie_val = "__cf_bm=fZ28nhLbpGjyMx9OLnQSSImuGJOgK1DXXNuZveJ3lDE-1772371369.3022616-1.0.1.1-tD2S58jQJqbB3dK3FVP6m5IxINPUA_SvLL.Da.WnJhBFncPnb1dD2zQEoqQelooV2s.u5EC4.iZx1A7KFmj98Th2F7e8OpdMjact1fO1Jr6yVYtw8Q5xf.zfacwPu1zzojM_2Zllk9amHQh4yDm_Lg; bl_csrf=50907dc9fe215b9683a70a9ee370365a; bl_session=e0268778b16663835e86af7d5e2deb4d; __cflb=04dToRCegghj9KSg7BqsUc4efEezbNiLBwtaG3wvgM";
    headers.insert(reqwest::header::COOKIE, reqwest::header::HeaderValue::from_static(cookie_val));

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .ok()?;
        
    let resp = client.get(&url).send().await.ok()?;
    
    if !resp.status().is_success() {
        println!(">>> [Scraper] Failed: HTTP {}", resp.status());
        return None;
    }
    
    let html = resp.text().await.ok()?;
    
    // Check for "Bad Barcode"
    if html.contains("<title>Bad Barcode") {
        println!(">>> [Scraper] Failed: Bad Barcode page");
        return None;
    }
    
    // Extract Name
    static RE_NAME: OnceLock<Regex> = OnceLock::new();
    let re_name = RE_NAME.get_or_init(|| Regex::new(r"(?s)<h4>(.*?)</h4>").unwrap());
    
    let name_raw = if let Some(cap) = re_name.captures(&html) {
        cap[1].trim().to_string()
    } else {
        println!(">>> [Scraper] Failed: No name found in HTML");
        return None; // Name is critical, fail if not found
    };
    
    let name = clean_product_name(&name_raw);
    if name.is_empty() { 
        println!(">>> [Scraper] Failed: Name was empty after cleaning");
        return None; 
    }

    // Extract Type
    static RE_CAT: OnceLock<Regex> = OnceLock::new();
    let re_cat = RE_CAT.get_or_init(|| Regex::new(r"(?s)Category:\s*<span class=.product-text.>(.*?)</span>").unwrap());
    
    let type_ = if let Some(cap) = re_cat.captures(&html) {
        parse_category(&cap[1])
    } else {
        8 // Default to "other"
    };

    // Extract Description
    static RE_DESC: OnceLock<Regex> = OnceLock::new();
    let re_desc = RE_DESC.get_or_init(|| Regex::new(r"(?s)Description:(?:(?:&nbsp;)|(?:&#160;)|\s)*<span class=.product-text.>(.*?)</span>").unwrap());
    
    let desc = if let Some(cap) = re_desc.captures(&html) {
        let d = cap[1].trim();
        if d.is_empty() { None } else { Some(d.replace("&quot;", "\"").replace("&amp;", "&")) }
    } else {
        None
    };

    // Extract Image URL
    static RE_IMG: OnceLock<Regex> = OnceLock::new();
    let re_img = RE_IMG.get_or_init(|| Regex::new(r"(?s)<div id=.largeProductImage.>\s*<img src=.(.*?).\s+alt").unwrap());
    
    let image_url = if let Some(cap) = re_img.captures(&html) {
        Some(cap[1].to_string())
    } else {
        None
    };

    println!(">>> [Scraper] Success: Extracted product '{}' (Category: {})", name, type_);

    Some(ScrapedProduct {
        name,
        type_,
        desc,
        image_url,
    })
}

pub async fn download_image(url: &str, image_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?;
    if resp.status().is_success() {
        let bytes = resp.bytes().await?;
        let path = format!("static/images/{}", image_id);
        std::fs::create_dir_all("static/images")?;
        std::fs::write(path, bytes)?;
    }
    Ok(())
}
