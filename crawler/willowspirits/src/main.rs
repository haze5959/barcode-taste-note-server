use dotenvy::dotenv;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

// ============================================================
// Wix/WillowSpirits 메타 변수에서 파싱할 제품 모델
// ============================================================

#[derive(Debug, Deserialize)]
struct WillowDocument {
    title: Option<String>,
    image: Option<WillowImage>,
}

#[derive(Debug, Deserialize)]
struct WillowImage {
    name: String,
}

// ============================================================
// Gemini 요청/응답 모델
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
// 출력 JSON 모델
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
// Gemini 텍스트 호출 (productName → product_name + desc)
// ============================================================

const GEMINI_PROMPT_TEMPLATE: &str = r#"Analyze F&B product name. If NOT F&B, return: {"error":"Not an F&B product"}.
Name: Core English name ONLY. EXCLUDE promo/limited/seasonal/capacity/containers(Can,Bottle,Box,etc)/category suffixes(Whisky,Wine,Beer,etc). No hyphens. Title Case. KEEP aging as "X Years Old" (e.g., 7YO/7yo/7 year old → "7 Years Old"). EXCLUDE vintage year from product name for wine (e.g., exclude 2019, 2020). KEEP brand prefix if name alone is just a flavor/color/descriptor (e.g., "Cherry Liqueur" → "Quaglia Cherry", "Pistachio Cream" → "Cellini Crema Di Pistacchio").
Desc: Professional factual English desc (<200 chars). No repeating name. Include production methods, flavor markers, market specs.
Return JSON: {"product_name":"...","desc":"...","details":{"style":<int>,"manufacturer":"<str>","country":"<2-letter_iso>","alcohol":<float>,"grape":<int>,"ibu":<int>}}
Rules for 'details': 'grape' ONLY if wine. 'ibu' ONLY if beer. Use null for any field you are not confident about.
STYLE: Wine(0:red,1:white,2:rose,3:sparkling,4:dessert,5:fortified,6:natural),Whisky(100:singleMalt,101:blended,102:singleGrain,103:bourbon,104:rye,105:tennessee,106:irish,107:japanese,108:canadian,109:other),Beer(200:lager,201:pilsner,202:paleAle,203:ipa,204:hazyIpa,205:stout,206:porter,207:wheat,208:sour,209:belgianAle,210:amber),Asian(300:soju,301:fruitSoju,302:junmai,303:junmaiGinjo,304:junmaiDaiginjo,305:ginjo,306:daiginjo,307:honjozo,308:nigori,309:cheongju,310:yakju,311:makgeolli),Spirits(400:vodka,401:gin,402:lightRum,403:darkRum,404:spicedRum,405:tequila,406:mezcal,407:brandy,408:cognac,409:armagnac,410:absinthe,411:baijiu,412:liqueur),Cocktail(500:classic,501:craft,502:tiki,503:sour,504:highball,505:frozen,506:mocktail),Coffee(600:espresso,601:americano,602:latte,603:cappuccino,604:macchiato,605:flatWhite,606:mocha,607:drip,608:pourOver,609:coldBrew,610:singleOrigin),Other(700:other)
GRAPE(Wine ONLY): Red(0:cabSauv,1:merlot,2:pinotNoir,3:syrah,4:malbec,5:sangiovese,6:tempranillo,7:nebbiolo,8:grenache,9:zinfandel,10:cabFranc,11:carmenere,12:gamay,13:montepulciano,14:petitVerdot),White(100:chardonnay,101:sauvBlanc,102:riesling,103:pinotGrigio,104:gewurztraminer,105:cheninBlanc,106:viognier,107:semillon,108:moscato,109:albarino,110:pinotBlanc),Other(200:redBlend,201:whiteBlend,299:other)
Product name: "{}""#;

async fn call_gemini(client: &reqwest::Client, api_key: &str, product_name: &str) -> Result<(String, String, Option<serde_json::Value>), String> {
    let prompt = GEMINI_PROMPT_TEMPLATE.replace("{}", product_name);

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart { text: prompt }],
        }],
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent?key={}",
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

    // JSON 블록 정리 (```json ... ``` 제거)
    let cleaned = text_output
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // error 케이스 확인
    if cleaned.contains("\"error\"") {
        return Err(format!("Gemini returned error for: {}", product_name));
    }

    // product_name, desc 파싱
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
// HTML 파싱: 문서에서 제품 정보 추출
// ============================================================

fn parse_documents(html: &str) -> Vec<WillowDocument> {
    let re = Regex::new(r#""search:SearchResponse"\s*:\s*\{"documents"\s*:\s*"#).unwrap();
    if let Some(mat) = re.find(html) {
        let bytes = html.as_bytes();
        let mut json_start = mat.end();
        while json_start < bytes.len() && bytes[json_start] != b'[' {
            json_start += 1;
        }
        if json_start >= bytes.len() {
            println!("  '[' NOT FOUND AFTER KEY");
            return vec![];
        }

        let mut depth = 0;
        let mut json_end = json_start;
        for (i, &b) in bytes[json_start..].iter().enumerate() {
            match b {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        json_end = json_start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        let json_str = &html[json_start..json_end];
        match serde_json::from_str::<Vec<WillowDocument>>(json_str) {
            Ok(docs) => return docs,
            Err(e) => {
                eprintln!("  JSON 파싱 실패: {}", e);
                eprintln!("  JSON raw: {}", &json_str[..std::cmp::min(json_str.len(), 200)]);
            }
        }
    } else {
        println!("  'search:SearchResponse' KEY NOT FOUND IN HTML");
    }
    vec![]
}

// ============================================================
// MAIN
// ============================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap());
            h.insert(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9".parse().unwrap());
            h
        })
        .build()?;

    let mut all_results: Vec<OutputProduct> = Vec::new();
    let base_url = "https://www.willowspirits.com/search?type=products";
    let barcode_re = Regex::new(r"\(([\d]{8,15})\)").unwrap();

    let mut page_num: u32 = 1;
    let mut consecutive_empty = 0;

    println!("\n========================================");
    println!("Willow Spirits 크롤링 시작");
    println!("========================================");

    loop {
        let url = format!("{}&page={}", base_url, page_num);
        println!("\n[Page {}] Fetching: {}", page_num, url);

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[Page {}] Request error: {}", page_num, e);
                consecutive_empty += 1;
                if consecutive_empty >= 3 {
                    println!("3 consecutive errors. Stopping collection.");
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
            if consecutive_empty >= 3 { break; }
            sleep(Duration::from_secs(2)).await;
            page_num += 1;
            continue;
        }

        let html = resp.text().await.unwrap_or_default();
        let docs = parse_documents(&html);

        if docs.is_empty() {
            println!("[Page {}] 제품 없음 → 크롤링 종료", page_num);
            break;
        }

        consecutive_empty = 0;
        println!("[Page {}] {}개 제품 발견", page_num, docs.len());

        for doc in docs.iter() {
            let title = match &doc.title {
                Some(t) => t,
                None => {
                    println!("  → title 필드 없음, 스킵");
                    continue;
                }
            };
            let barcode = match barcode_re.captures(title) {
                Some(cap) => cap[1].to_string(),
                None => {
                    println!("  → 바코드 없음, 스킵: {}", title);
                    continue;
                }
            };

            let clean_title = barcode_re.replace_all(title, "").to_string().trim().to_string();

            let image_url = match &doc.image {
                Some(img) => {
                    format!("https://static.wixstatic.com/media/{}/v1/fill/w_440,h_440,al_c,lg_1,q_80,enc_auto/{}", img.name, img.name)
                },
                None => {
                    println!("  → 이미지 없음, 스킵: {}", title);
                    continue;
                }
            };

            let type_ = "spirit".to_string();

            // Gemini 호출
            match call_gemini(&client, &api_key, &clean_title).await {
                Ok((product_name, desc, details)) => {
                    println!("  ✓ {} → {}", clean_title, product_name);
                    all_results.push(OutputProduct {
                        barcode,
                        product_name,
                        desc,
                        type_,
                        image_url: Some(image_url),
                        details,
                    });
                }
                Err(e) => {
                    println!("  ✗ {} 실패: {}", clean_title, e);
                }
            }

            // API 부하 방지
            sleep(Duration::from_millis(500)).await;
        }

        // 결과 중간 저장 (각 페이지 완료 후)
        save_output(&all_results);

        page_num += 1;
        sleep(Duration::from_secs(1)).await;
    }

    println!("\n완료. 총 {}개 제품 저장됨.", all_results.len());
    save_output(&all_results);

    Ok(())
}

fn save_output(products: &[OutputProduct]) {
    let json_str = match serde_json::to_string_pretty(products) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("JSON 직렬화 실패: {}", e);
            return;
        }
    };

    if let Err(e) = std::fs::write("output/new_product.json", &json_str) {
        // output 폴더 없으면 생성 후 재시도
        let _ = std::fs::create_dir_all("output");
        if let Err(e2) = std::fs::write("output/new_product.json", &json_str) {
            eprintln!("파일 저장 실패: {} / {}", e, e2);
        }
    }
}
