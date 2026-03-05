fn main() {
    let name = "KIRIN'S PRIME BREW . ALUMINUM PULL TAB BEER CAN from 2014";
    let re_spam = regex::Regex::new(r"(?i)\b(empty|can only|no drink|used|aluminum|pull tab|beer can|from \d{4})\b").unwrap();
    let mut cleaned = re_spam.replace_all(name, " ").to_string();
    let re_spaces = regex::Regex::new(r"\s{2,}").unwrap();
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();
    println!("cleaned: '{}'", cleaned);
    cleaned = cleaned.trim().trim_end_matches(&[',', '-', ' ', '.'][..]).trim().to_string();
    println!("final: '{}'", cleaned);
}
