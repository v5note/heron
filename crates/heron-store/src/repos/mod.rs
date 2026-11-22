//! Repositories. Each module is a thin, testable layer over SQL — no business rules.

pub mod client;
pub mod detected;
pub mod event;
pub mod projection;
pub mod skill;
pub mod source;
pub mod target;
pub mod visibility;

pub(crate) fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_owned())
}

pub(crate) fn parse_list(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}
