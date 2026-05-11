use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct ScrapedProduct {
    pub name: String,
    pub type_: i16,
    pub desc: Option<String>,
    pub image_url: Option<String>,
    pub details: Option<serde_json::Value>,
}

pub fn parse_category(tags_str: &str) -> i16 {
    let lower = tags_str.to_lowercase();
    if lower.contains("wine") { return 0; }
    if lower.contains("whisky") || lower.contains("whiskey") || lower.contains("whiskies") { return 1; }
    if lower.contains("beer") { return 2; }
    if lower.contains("soju") || lower.contains("sake") { return 3; }
    if lower.contains("liqueur") || lower.contains("liquor") || lower.contains("spirit") { return 4; }
    if lower.contains("cocktail") { return 5; }
    if lower.contains("coffee") { return 6; }
    if lower.contains("beverage") { return 7; }
    8
}

fn clean_product_name(name: &str) -> String {
    let mut cleaned = name.replace("&quot;", "\"").replace("&amp;", "&");
    
    static RE_PARENS: OnceLock<Regex> = OnceLock::new();
    let re_parens = RE_PARENS.get_or_init(|| Regex::new(r"\(.*?\)").unwrap());
    cleaned = re_parens.replace_all(&cleaned, " ").to_string();
    
    static RE_YEARS: OnceLock<Regex> = OnceLock::new();
    let re_years = RE_YEARS.get_or_init(|| Regex::new(r"(?i)\b(?:aged\s+)?(\d+)\s*(?:years?(?:\s*old)?|y\.?o\.?)\b").unwrap());
    cleaned = re_years.replace_all(&cleaned, "${1} Years Old").to_string();
    
    static RE_ABV: OnceLock<Regex> = OnceLock::new();
    let re_abv = RE_ABV.get_or_init(|| Regex::new(r"(?i)\d+(\.\d+)?\s*%\s*(vol\.?)?").unwrap());
    cleaned = re_abv.replace_all(&cleaned, " ").to_string();
    
    static RE_MEASURE: OnceLock<Regex> = OnceLock::new();
    let re_measure = RE_MEASURE.get_or_init(|| Regex::new(r"(?i)\b\d+(\.\d+)?\s*(ml|cl|lt|l|liter|liters|litre|litres|g|kg|mg|oz|fl\.?\s*oz|lb|lbs)\b").unwrap());
    cleaned = re_measure.replace_all(&cleaned, " ").to_string();
    
    static RE_QTY: OnceLock<Regex> = OnceLock::new();
    let re_qty = RE_QTY.get_or_init(|| Regex::new(r"(?i)\b(x\s*\d+\s*(pcs|pack|packs|ea)?|\d+\s*(pcs|pack|packs|ea|bottles|cans))\b").unwrap());
    cleaned = re_qty.replace_all(&cleaned, " ").to_string();
    
    static RE_SPAM: OnceLock<Regex> = OnceLock::new();
    let re_spam = RE_SPAM.get_or_init(|| Regex::new(r"(?i)\b(empty|can only|no drink|used|aluminum|pull tab|beer can|from \d{4})\b").unwrap());
    cleaned = re_spam.replace_all(&cleaned, " ").to_string();

    // 주류 종류 및 용기, 원산지 스팸 단어 제거
    static RE_LIQUOR_TERMS: OnceLock<Regex> = OnceLock::new();
    let re_liquor = RE_LIQUOR_TERMS.get_or_init(|| {
        Regex::new(r"(?i)\b(red wine|white wine|rose wine|sparkling wine|wine|whiskey|whisky|bourbon|scotch|vodka|gin|rum|tequila|cognac|brandy|liqueur|beer|soju|sake|cocktail|bottle|bottles|can|cans|glass|from california|from france|from italy|from spain|from australia|from chile|from argentina)\b").unwrap()
    });
    cleaned = re_liquor.replace_all(&cleaned, " ").to_string();
    
    static RE_SYMBOLS: OnceLock<Regex> = OnceLock::new();
    let re_symbols = RE_SYMBOLS.get_or_init(|| Regex::new(r"[-_—|/·,]").unwrap());
    cleaned = re_symbols.replace_all(&cleaned, " ").to_string();

    static RE_SPACES: OnceLock<Regex> = OnceLock::new();
    let re_spaces = RE_SPACES.get_or_init(|| Regex::new(r"\s{2,}").unwrap());
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();

    // 이름 끝에 남는 단독 숫자·개수·용량 패턴 반복 제거
    // 예: "6 pack", "x1", "1 LT", ", 5", ", 0" 등
    static RE_TRAILING: OnceLock<Regex> = OnceLock::new();
    let re_trailing = RE_TRAILING.get_or_init(|| {
        Regex::new(r"(?i)\s+(x\s*\d+|\d+\s*(pack|packs|lt|l|ml|cl|g|kg|oz|pcs|ea|bottles|cans)?|\d+)$").unwrap()
    });
    loop {
        let next = re_trailing.replace(&cleaned, "").to_string();
        let next = next.trim().trim_end_matches(&[',', '-', ' ', '.'][..]).trim().to_string();
        if next == cleaned.trim() {
            break;
        }
        cleaned = next;
    }

    // Remove trailing commas, hyphens, spaces, or dots
    let trimmed = cleaned.trim().trim_end_matches(&[',', '-', ' ', '.'][..]).trim().to_string();

    // Title Case 변환: 각 단어의 첫 글자를 대문자로
    trimmed
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    // Extract Type and Description using Gemini
    let (type_, desc, details) = if let Some(info) = crate::utils::gemini::generate_product_info_with_gemini(&name).await {
        (parse_category(&info.category), Some(info.description), info.details)
    } else {
        (8, None, None)
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
        details,
    })
}

pub async fn download_image(r2: &crate::utils::r2::R2Client, url: &str, image_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?;
    
    if !resp.status().is_success() {
        return Err(format!("Download failed with status: {}", resp.status()).into());
    }

    let bytes = resp.bytes().await?;
    
    // Validate that the bytes represent a valid image
    let img = image::load_from_memory(&bytes).map_err(|e| {
        format!("Invalid image file from {}: {}", url, e)
    })?;

    let final_bytes = if bytes.len() > 100_000 {
        let resized = img.resize(400, 400, image::imageops::FilterType::Nearest);
        let mut buffer = std::io::Cursor::new(Vec::new());
        resized.write_to(&mut buffer, image::ImageFormat::Jpeg)?;
        buffer.into_inner()
    } else {
        let mut buffer = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buffer, image::ImageFormat::Jpeg)?;
        buffer.into_inner()
    };

    let key = format!("images/{}", image_id);
    r2.upload_image(&key, final_bytes, "image/jpeg").await.map_err(|e| {
        format!("R2 upload failed: {:?}", e)
    })?;

    Ok(())
}

/// DuckDuckGo 이미지 검색으로 제품명 검색 후 첫 번째 이미지 URL 반환
pub async fn search_duckduckgo_image_url(product_name: &str) -> Option<String> {
    static VQD_RE: OnceLock<Regex> = OnceLock::new();
    let vqd_re = VQD_RE.get_or_init(|| Regex::new(r#"vqd=["']?([\d-]+)["']?"#).unwrap());

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .build()
        .ok()?;

    // Step 1: vqd 토큰 취득
    let init_resp = client
        .get("https://duckduckgo.com/")
        .query(&[("q", product_name), ("iax", "images"), ("ia", "images")])
        .send()
        .await
        .ok()?;

    let html = init_resp.text().await.ok()?;
    
    // vqd 추출 실패 시 로그를 남겨서 나중에 DDG 정책이 바뀌었을 때 대처 가능하게 함
    let vqd = match vqd_re.captures(&html).and_then(|cap| cap.get(1)) {
        Some(m) => m.as_str().to_string(),
        None => {
            log::warn!("[DuckDuckGo Image Search] vqd 토큰 추출 실패. HTML 구조가 변경되었거나 차단되었을 수 있습니다.");
            return None;
        }
    };

    // Step 2: 이미지 JSON API 호출
    let img_resp = client
        .get("https://duckduckgo.com/i.js")
        .query(&[
            ("q", product_name),
            ("o", "json"),
            ("l", "us-en"),
            ("vqd", vqd.as_str()),
            ("f", ",,,"),
            ("p", "1"),
        ])
        .send()
        .await
        .ok()?;

    if !img_resp.status().is_success() {
        log::error!("[DuckDuckGo Image Search] API error: {}", img_resp.status());
        return None;
    }

    let json: serde_json::Value = match img_resp.json().await {
        Ok(j) => j,
        Err(e) => {
            log::error!("[DuckDuckGo Image Search] JSON 파싱 에러: {}", e);
            return None;
        }
    };

    // Step 3: 패닉 방지를 위한 안전한 JSON 접근 (get 메서드 체이닝)
    json.get("results")
        .and_then(|results| results.as_array())
        .and_then(|arr| arr.first()) // 배열이 비어있으면 None 반환
        .and_then(|first_item| first_item.get("image"))
        .and_then(|img| img.as_str())
        .map(|s| s.to_string())
}