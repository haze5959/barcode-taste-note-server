use uuid::Uuid;

const ADJECTIVES_KO: &[&str] = &[
    "달콤한", "고독한", "향긋한", "쌉싸름한", "상큼한", "짜릿한", "부드러운", "신비로운",
    "은은한", "화려한", "투명한", "시원한", "따뜻한", "경쾌한", "묵직한", "달곰한",
    "우아한", "깊은", "아늑한", "밝은", "차분한", "강렬한", "선명한", "클래식한",
    "비밀스러운", "몽환적인", "스모키한", "풍부한", "아로마틱한", "금빛의", "어두운", "스파이시한"
];
const NOUNS_KO: &[&str] = &[
    "위스키", "바텐더", "테이스터", "칵테일", "보드카", "레몬", "체리", "올리브",
    "오크통", "바닐라", "얼음", "글라스", "와인", "코냑", "진", "럼",
    "데킬라", "민트", "라임", "시럽", "소금", "후추", "커피", "카카오",
    "지거", "쉐이커", "보틀", "코르크", "포도", "복숭아", "사과", "꿀"
];

const ADJECTIVES_EN: &[&str] = &[
    "Sweet", "Lonely", "Fragrant", "Bitter", "Fresh", "Thrilling", "Smooth", "Mystic",
    "Subtle", "Fancy", "Clear", "Cool", "Warm", "Joyful", "Heavy", "Mellow",
    "Elegant", "Deep", "Cozy", "Bright", "Chill", "Strong", "Vivid", "Classic",
    "Secret", "Dreamy", "Smoky", "Rich", "Aromatic", "Golden", "Dark", "Spicy"
];
const NOUNS_EN: &[&str] = &[
    "Whiskey", "Bartender", "Taster", "Cocktail", "Vodka", "Lemon", "Cherry", "Olive",
    "Oak", "Vanilla", "Ice", "Glass", "Wine", "Cognac", "Gin", "Rum",
    "Tequila", "Mint", "Lime", "Syrup", "Salt", "Pepper", "Coffee", "Cacao",
    "Jigger", "Shaker", "Bottle", "Cork", "Grape", "Peach", "Apple", "Honey"
];

const ADJECTIVES_JA: &[&str] = &[
    "甘い", "孤独な", "香り高い", "ほろ苦い", "爽やかな", "刺激的な", "滑らかな", "神秘的な",
    "仄かな", "華やかな", "透明な", "涼しい", "温かい", "軽快な", "重厚な", "芳醇な",
    "エレガントな", "深い", "心地よい", "明るい", "落ち着いた", "強烈な", "鮮やかな", "クラシックな",
    "秘密の", "夢幻的な", "スモーキーな", "豊かな", "アロマティックな", "金色の", "暗い", "スパイシーな"
];
const NOUNS_JA: &[&str] = &[
    "ウイスキー", "バーテンダー", "テイスター", "カクテル", "ウォッカ", "レモン", "チェリー", "オリーブ",
    "オーク樽", "バニラ", "氷", "グラス", "ワイン", "コニャック", "ジン", "ラム",
    "テキーラ", "ミント", "ライム", "シロップ", "塩", "胡椒", "コーヒー", "カカオ",
    "ジガー", "シェイカー", "ボトル", "コルク", "ブドウ", "ピーチ", "リンゴ", "ハチミツ"
];

const ADJECTIVES_ZH: &[&str] = &[
    "甜蜜的", "孤独的", "芬芳的", "苦涩的", "清新的", "刺激的", "顺滑的", "神秘的",
    "淡淡的", "华丽的", "透明的", "凉爽的", "温暖的", "轻快的", "厚重的", "醇厚的",
    "优雅的", "深沉的", "舒适的", "明亮的", "平静的", "强烈的", "鲜艳的", "经典的",
    "秘密的", "梦幻的", "烟熏的", "丰富的", "芳香的", "金色的", "黑暗的", "辛辣的"
];
const NOUNS_ZH: &[&str] = &[
    "威士忌", "调酒师", "品酒师", "鸡尾酒", "伏特加", "柠檬", "樱桃", "橄榄",
    "橡木桶", "香草", "冰块", "酒杯", "葡萄酒", "干邑", "金酒", "朗姆酒",
    "龙舌兰", "薄荷", "青柠", "糖浆", "盐", "胡椒", "咖啡", "可可",
    "量酒器", "摇酒壶", "瓶子", "软木塞", "葡萄", "桃子", "苹果", "蜂蜜"
];

// 간단한 자체 해시(Base62) 인코더로 영어 대소문자+숫자인 짧은 식별자 생성
fn encode_base62(mut num: u64, length: usize) -> String {
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut result = String::with_capacity(length);
    for _ in 0..length {
        result.push(chars[(num % 62) as usize] as char);
        num /= 62;
    }
    result
}

// UUID에서 난수를 추출해 단어 인덱스를 계산하고 식별자를 붙이는 방식
pub fn generate_nickname(locale: &str, user_id: &Uuid) -> String {
    let u = user_id.as_u128();
    
    let adj_idx = ((u >> 64) % 32) as usize;
    let noun_idx = ((u >> 32) % 32) as usize;
    
    // UUID의 하위 64비트를 사용해 3글자 식별자(해시) 생성
    let identifier = encode_base62(u as u64, 3);
    
    if locale.starts_with("ko") {
        format!("{}_{}_{}", ADJECTIVES_KO[adj_idx], NOUNS_KO[noun_idx], identifier)
    } else if locale.starts_with("ja") {
        format!("{}_{}_{}", ADJECTIVES_JA[adj_idx], NOUNS_JA[noun_idx], identifier)
    } else if locale.starts_with("zh") {
        format!("{}_{}_{}", ADJECTIVES_ZH[adj_idx], NOUNS_ZH[noun_idx], identifier)
    } else {
        format!("{}_{}_{}", ADJECTIVES_EN[adj_idx], NOUNS_EN[noun_idx], identifier)
    }
}
