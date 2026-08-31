//! Thin objc2 shim over NSUserDefaults. Deliberately logic-free: everything
//! that decides anything lives in `state.rs`, where it is unit-tested.

use objc2_foundation::{NSArray, NSString, NSUserDefaults};

pub const HISTORY_KEY: &str = "MDViewHistory";
#[allow(dead_code)]
pub const BOOKMARKS_KEY: &str = "MDViewBookmarks";
#[allow(dead_code)]
pub const THEME_KEY: &str = "MDViewTheme";
#[allow(dead_code)]
pub const SIDEBAR_OPEN_KEY: &str = "MDViewSidebarOpen";
#[allow(dead_code)]
pub const SIDEBAR_TAB_KEY: &str = "MDViewSidebarTab";
#[allow(dead_code)]
pub const FULL_WIDTH_KEY: &str = "MDViewFullWidth";
#[allow(dead_code)]
pub const DIFF_LAYOUT_KEY: &str = "MDViewDiffLayout";
#[allow(dead_code)]
pub const SIDEBAR_WIDTH_KEY: &str = "MDViewSidebarWidth";
#[allow(dead_code)]
pub const SHORTCUTS_HINT_SHOWN_KEY: &str = "MDViewShortcutsHintShown";

fn defaults() -> objc2::rc::Retained<NSUserDefaults> {
    NSUserDefaults::standardUserDefaults()
}

pub fn get_strings(key: &str) -> Vec<String> {
    let key = NSString::from_str(key);
    match defaults().stringArrayForKey(&key) {
        Some(array) => array.iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

pub fn set_strings(key: &str, values: &[String]) {
    let key = NSString::from_str(key);
    let items: Vec<_> = values.iter().map(|s| NSString::from_str(s)).collect();
    let refs: Vec<&NSString> = items.iter().map(|s| &**s).collect();
    let array = NSArray::from_slice(&refs);
    unsafe { defaults().setObject_forKey(Some(&array), &key) };
}

#[allow(dead_code)]
pub fn get_string(key: &str) -> Option<String> {
    let key = NSString::from_str(key);
    defaults().stringForKey(&key).map(|s| s.to_string())
}

#[allow(dead_code)]
pub fn set_string(key: &str, value: &str) {
    let key = NSString::from_str(key);
    let value = NSString::from_str(value);
    unsafe { defaults().setObject_forKey(Some(&value), &key) };
}

#[allow(dead_code)]
pub fn get_bool(key: &str) -> bool {
    let key = NSString::from_str(key);
    defaults().boolForKey(&key)
}

/// `Some(value)` when the key is present, `None` when it has never been set.
/// `get_bool` cannot express that difference, and a default of "open" is only
/// correct until the user has expressed a preference.
#[allow(dead_code)]
pub fn get_bool_opt(key: &str) -> Option<bool> {
    let key = NSString::from_str(key);
    let defaults = defaults();
    defaults.objectForKey(&key).map(|_| defaults.boolForKey(&key))
}

#[allow(dead_code)]
pub fn set_bool(key: &str, value: bool) {
    let key = NSString::from_str(key);
    defaults().setBool_forKey(value, &key);
}

#[allow(dead_code)]
pub fn get_int_opt(key: &str) -> Option<i64> {
    let key = NSString::from_str(key);
    let defaults = defaults();
    defaults
        .objectForKey(&key)
        .map(|_| defaults.integerForKey(&key) as i64)
}

#[allow(dead_code)]
pub fn set_int(key: &str, value: i64) {
    let key = NSString::from_str(key);
    defaults().setInteger_forKey(value as isize, &key);
}
