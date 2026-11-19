//! Human-readable rendering. Structured (--json) output is always produced elsewhere.

use heron_scan::classify::HitKind;
use heron_scan::walker::ScanResult;

pub fn print_scan(result: &ScanResult) {
    let skills = result
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::Skill)
        .count();
    let legacy = result
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::LegacyCommand)
        .count();
    let nested = result.nested.len();
    let flagged = result.hits.iter().filter(|h| !h.frontmatter_ok()).count();

    println!(
        "scanned {} directories in {} ms",
        result.stats.dirs_visited, result.stats.duration_ms
    );
    println!("  skills          {skills}");
    println!("  legacy commands {legacy}");
    println!("  nested suspects {nested}   (reported only - nothing is moved)");
    println!("  needs attention {flagged}");
    for missing in &result.stats.roots_missing {
        println!(
            "  skipped (not found) {}",
            heron_platform::paths::display(missing)
        );
    }
    if result.stats.truncated {
        println!("  ! scan limit reached; raise scan.max_dirs to see everything");
    }
}

pub fn print_hits(result: &ScanResult, show_all: bool) {
    for hit in &result.hits {
        if !show_all && hit.kind == HitKind::NestedSuspect {
            continue;
        }
        let kind = match hit.kind {
            HitKind::Skill => "skill",
            HitKind::LegacyCommand => "command",
            HitKind::NestedSuspect => "nested",
        };
        let client = hit.client.clone().unwrap_or_else(|| "-".to_owned());
        let scope = match hit.scope {
            Some(heron_scan::classify::Scope::Project) => "project",
            Some(heron_scan::classify::Scope::Global) => "global",
            None => "-",
        };
        let name = hit.slug.clone().unwrap_or_else(|| "?".to_owned());
        let flag = if hit.frontmatter_ok() {
            ""
        } else {
            "  [needs attention]"
        };
        println!(
            "{name:<28} {client:<9} {scope:<8} {kind:<8} {}{flag}",
            heron_platform::paths::display(&hit.path)
        );
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_formatting() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
    }
}
