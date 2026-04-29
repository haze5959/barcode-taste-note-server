use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::RwLock;
use chrono::{Utc, NaiveDate};

lazy_static! {
    // sub -> (count, last_access_date)
    static ref RATE_LIMIT_CACHE: RwLock<HashMap<String, (u32, NaiveDate)>> = RwLock::new(HashMap::new());
}

pub fn check_and_increment_api_usage(sub: &str, limit: u32) -> bool {
    let today = Utc::now().date_naive();
    
    // Write lock to safely update map
    let mut cache = match RATE_LIMIT_CACHE.write() {
        Ok(c) => c,
        Err(_) => return false, // If poisoned, fail closed or open. Let's fail for safety or restart.
    };

    let entry = cache.entry(sub.to_string()).or_insert((0, today));

    // Reset if it's a new day
    if entry.1 != today {
        entry.0 = 0;
        entry.1 = today;
    }

    if entry.0 >= limit {
        return false;
    }

    entry.0 += 1;
    true
}
