//! How much of a repository a skill actually needs.
//!
//! Most skills travel alone: the directory holding `SKILL.md` contains everything they use, so
//! `heron install` can export just that subtree (`SkillLayout::Skill`). Some repositories are
//! built the other way round — `alchaincyf/huashu-motion` ships `skills/huashu-motion/bin/om`,
//! a shim that runs `cd "$(dirname "$0")/../../.." && exec node cli/om.js`, so the skill is only
//! usable *inside* the checkout: `cli/`, `runtime/`, `templates/` and the asset directories have
//! to sit around it. Exporting the subtree alone leaves that shim pointing at nothing.
//!
//! For those we materialize the whole repository (`SkillLayout::Repo`) and address the skill at
//! its subpath inside it, which is exactly what a user gets from `git clone` — the recommended
//! install in the project's own README.

use std::collections::HashSet;

/// What a checkout of `skill_dir` has to contain for the skill to work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLayout {
    /// The skill directory is self-contained: export `skill_dir` and make it the store entry.
    Skill,
    /// The skill needs the repository around it: export the whole tree, the skill sits at
    /// `skill_dir` inside the checkout.
    Repo,
}

impl SkillLayout {
    pub fn whole_repo(self) -> bool {
        matches!(self, SkillLayout::Repo)
    }
}

/// Manifests at the repository root: evidence the tree around a skill is a real project and not
/// an accident of layout.
const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "setup.py",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Gemfile",
];

/// Extensions worth reading for path references. Anything else (images, audio, video) cannot
/// carry one we can parse.
const SCAN_SUFFIXES: &[&str] = &[
    "md", "txt", "sh", "bash", "zsh", "js", "mjs", "cjs", "ts", "tsx", "py", "rb", "go", "rs",
    "json", "toml", "yaml", "yml",
];

/// Blob cap per file, and a cap on how many files we read at all: detection runs on every
/// preview, and a skill directory is a handful of files, not a monorepo walk.
const MAX_SCAN_BYTES: usize = 256 * 1024;
const MAX_SCAN_FILES: usize = 80;

/// Decide how `skill_dir` should be checked out.
///
/// `files` is every path in the repository at the chosen commit (blobs only), `read` returns the
/// text of a path or `None` for binary/unreadable content.
pub fn detect(
    skill_dir: &str,
    files: &[String],
    read: &dyn Fn(&str) -> Option<String>,
) -> SkillLayout {
    let skill_dir = skill_dir.trim_matches('/');
    if skill_dir.is_empty() {
        // The repository root *is* the skill: there is nothing above it to depend on.
        return SkillLayout::Skill;
    }
    if let Some(declared) = declared(files, read) {
        return declared;
    }
    if escapes(skill_dir, files, read) {
        return SkillLayout::Repo;
    }
    if compound(skill_dir, files) {
        return SkillLayout::Repo;
    }
    SkillLayout::Skill
}

/// A repository can pin the answer, which is the escape hatch for layouts none of the rules
/// below can infer: `layout = "repo"` or `layout = "skill"` in `heron.source.toml`.
fn declared(files: &[String], read: &dyn Fn(&str) -> Option<String>) -> Option<SkillLayout> {
    if !files.iter().any(|p| p == "heron.source.toml") {
        return None;
    }
    let text = read("heron.source.toml")?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    match value.get("layout")?.as_str()?.trim() {
        "repo" => Some(SkillLayout::Repo),
        "skill" => Some(SkillLayout::Skill),
        _ => None,
    }
}

/// Does anything in the skill directory reach outside of it and land on a path the repository
/// really has? That reference is the proof the skill cannot travel alone.
fn escapes(skill_dir: &str, files: &[String], read: &dyn Fn(&str) -> Option<String>) -> bool {
    // Trees are not in `files` (`ls-tree -r` lists blobs), yet `../../templates` names one.
    // Every ancestor directory of every path is therefore a valid reference target.
    let mut known: HashSet<&str> = HashSet::new();
    for path in files {
        let mut head = path.as_str();
        loop {
            known.insert(head);
            match head.rfind('/') {
                Some(i) => head = &head[..i],
                None => break,
            }
        }
    }
    let prefix = format!("{skill_dir}/");
    let mut scanned = 0usize;
    for path in files.iter().filter(|p| p.starts_with(&prefix)) {
        if !scannable(path) || scanned >= MAX_SCAN_FILES {
            continue;
        }
        scanned += 1;
        let Some(text) = read(path) else { continue };
        if text.len() > MAX_SCAN_BYTES {
            continue;
        }
        for token in up_refs(&text) {
            let Some(target) = resolve(path, &token) else {
                continue;
            };
            if target == skill_dir || target.starts_with(&prefix) {
                continue;
            }
            if known.contains(target.as_str()) {
                return true;
            }
        }
    }
    false
}

fn scannable(path: &str) -> bool {
    match path.rsplit_once('.') {
        Some((_, ext)) => SCAN_SUFFIXES.contains(&ext),
        None => true,
    }
}

/// The `huashu-motion` shape: a single skill under a top-level `skills/` directory inside a
/// repository that is itself a project. Checked even when nothing escaped, because a compiled
/// shim or a path assembled at runtime is invisible to [`escapes`] — and pulling the whole
/// repository is the safe direction for a single-skill project.
fn compound(skill_dir: &str, files: &[String]) -> bool {
    let Some((parent, _)) = skill_dir.rsplit_once('/') else {
        return false;
    };
    if parent.contains('/') || (parent != "skills" && parent != "skill") {
        return false;
    }
    if files.iter().any(|p| p == "SKILL.md") {
        return false;
    }
    let under = format!("{parent}/");
    let siblings = files
        .iter()
        .filter_map(|p| p.strip_suffix("/SKILL.md"))
        .filter(|dir| dir.starts_with(&under) && *dir != skill_dir)
        .count();
    if siblings > 0 {
        // More than one skill under that directory: this is a skill *collection*, and each entry
        // is meant to be installed on its own.
        return false;
    }
    MANIFESTS.iter().any(|m| files.iter().any(|p| p == m))
}

/// Every `../..`-prefixed relative reference in `text`, as it appears.
fn up_refs(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        if chars[i] == '.' && chars[i + 1] == '.' && chars[i + 2] == '/' {
            let mut j = i;
            while j + 2 < chars.len()
                && chars[j] == '.'
                && chars[j + 1] == '.'
                && chars[j + 2] == '/'
            {
                j += 3;
            }
            let tail = j;
            while j < chars.len()
                && matches!(chars[j], 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '-' | '/')
            {
                j += 1;
            }
            if j > tail {
                out.push(chars[i..j].iter().collect());
            }
            i = tail.max(i + 3);
        } else {
            i += 1;
        }
    }
    out
}

/// Resolve `token` against the directory holding `file`. `None` when the reference climbs out of
/// the repository — no checkout can satisfy that, so it is not evidence of anything.
fn resolve(file: &str, token: &str) -> Option<String> {
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop()?;
    for seg in token.split('/') {
        match seg {
            ".." => {
                parts.pop()?;
            }
            "." | "" => {}
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("/");
    Some(joined.trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_nothing(_: &str) -> Option<String> {
        None
    }

    fn layout(skill_dir: &str, files: &[&str], texts: &[(&str, &str)]) -> SkillLayout {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        let texts: std::collections::HashMap<&str, &str> =
            texts.iter().map(|(k, v)| (*k, *v)).collect();
        detect(skill_dir, &files, &|p| texts.get(p).map(|s| s.to_string()))
    }

    #[test]
    fn a_self_contained_skill_stays_single_directory() {
        let l = layout(
            "skills/wechat-mp",
            &[
                "skills/wechat-mp/SKILL.md",
                "skills/wechat-mp/references/a.md",
                "README.md",
            ],
            &[("skills/wechat-mp/SKILL.md", "Run `pandoc` on the file.")],
        );
        assert_eq!(l, SkillLayout::Skill);
    }

    #[test]
    fn a_reference_outside_the_skill_pulls_the_whole_repository() {
        // The real huashu-motion shim: bin/om execs ../../../cli/om.js.
        let l = layout(
            "skills/huashu-motion",
            &[
                "skills/huashu-motion/SKILL.md",
                "skills/huashu-motion/bin/om",
                "cli/om.js",
                "package.json",
            ],
            &[(
                "skills/huashu-motion/bin/om",
                "DIR=$(cd \"$(dirname \"$0\")/../../..\" && pwd -P)\nexec node \"$DIR/cli/om.js\" \"$@\"\n",
            )],
        );
        assert_eq!(l, SkillLayout::Repo);
    }

    #[test]
    fn a_reference_that_only_walks_within_the_skill_is_no_evidence() {
        let l = layout(
            "skills/wechat-mp",
            &[
                "skills/wechat-mp/SKILL.md",
                "skills/wechat-mp/references/a.md",
            ],
            &[(
                "skills/wechat-mp/SKILL.md",
                "See [a](../wechat-mp/references/a.md) and `./scripts/run.sh`.",
            )],
        );
        assert_eq!(l, SkillLayout::Skill);
    }

    #[test]
    fn a_reference_to_a_path_the_repository_does_not_have_is_ignored() {
        // Prose about a sibling the author never committed must not force a whole-repo install.
        let l = layout(
            "skills/wechat-mp",
            &[
                "skills/wechat-mp/SKILL.md",
                "skills/wechat-mp/references/a.md",
            ],
            &[(
                "skills/wechat-mp/SKILL.md",
                "Compare with ../missing/tool.md first.",
            )],
        );
        assert_eq!(l, SkillLayout::Skill);
        let files = ["skills/wechat-mp/SKILL.md".to_owned()];
        assert!(!escapes("skills/wechat-mp", &files, &|_| {
            Some("see ../missing/tool.md".to_owned())
        }));
        let nested = ["skills/wechat-mp/sub/x".to_owned()];
        assert!(escapes("skills/wechat-mp", &nested, &|_| {
            Some("cd ../.. && ls".to_owned())
        }));
    }

    #[test]
    fn a_single_skill_project_directory_pulls_the_whole_repository() {
        // Same shape, but the tooling is referenced by a binary shim we cannot parse.
        let l = layout(
            "skills/tool",
            &[
                "skills/tool/SKILL.md",
                "skills/tool/bin/tool",
                "cli/main.js",
                "package.json",
            ],
            &[],
        );
        assert_eq!(l, SkillLayout::Repo);
    }

    #[test]
    fn a_collection_of_sibling_skills_installs_one_at_a_time() {
        let l = layout(
            "skills/wechat-mp",
            &[
                "skills/wechat-mp/SKILL.md",
                "skills/tdd/SKILL.md",
                "skills/wechat-mp/references/a.md",
                "package.json",
            ],
            &[("skills/wechat-mp/SKILL.md", "# wechat-mp\n")],
        );
        assert_eq!(l, SkillLayout::Skill);
    }

    #[test]
    fn a_deeply_nested_skill_directory_is_not_the_project_shape() {
        let l = layout(
            "packages/skills/wechat-mp",
            &[
                "packages/skills/wechat-mp/SKILL.md",
                "packages/skills/tdd/SKILL.md",
                "packages/package.json",
            ],
            &[],
        );
        assert_eq!(l, SkillLayout::Skill);
    }

    #[test]
    fn the_manifest_at_the_root_of_a_root_skill_does_not_matter() {
        let l = layout("root", &["SKILL.md", "package.json"], &[]);
        assert_eq!(l, SkillLayout::Skill);
    }

    #[test]
    fn heron_source_toml_overrides_both_directions() {
        let repo = layout(
            "skills/wechat-mp",
            &["skills/wechat-mp/SKILL.md", "heron.source.toml"],
            &[(
                "heron.source.toml",
                "layout = \"repo\"\n\n[[skill]]\npath = \"skills/wechat-mp\"\n",
            )],
        );
        assert_eq!(repo, SkillLayout::Repo);

        let skill = layout(
            "skills/tool",
            &[
                "skills/tool/SKILL.md",
                "skills/tool/bin/tool",
                "cli/main.js",
                "package.json",
                "heron.source.toml",
            ],
            &[(
                "heron.source.toml",
                "layout = \"skill\"\n\n[[skill]]\npath = \"skills/tool\"\n",
            )],
        );
        assert_eq!(skill, SkillLayout::Skill);
    }

    #[test]
    fn resolves_relative_paths_and_rejects_climbing_out() {
        assert_eq!(
            resolve("skills/p/bin/om", "../../../cli/om.js").as_deref(),
            Some("cli/om.js")
        );
        assert_eq!(
            resolve("skills/p/SKILL.md", "../wechat-mp").as_deref(),
            Some("skills/wechat-mp")
        );
        assert_eq!(resolve("skills/p/SKILL.md", "../.."), None);
    }

    #[test]
    fn collects_up_references_from_prose() {
        assert_eq!(
            up_refs("run ../../cli/om.js then ../../.. again"),
            vec!["../../cli/om.js".to_owned(), "../../..".to_owned()]
        );
        assert!(up_refs("no escapes here").is_empty());
    }

    #[test]
    fn unreadable_files_never_break_detection() {
        let files = vec![
            "skills/wechat-mp/SKILL.md".to_owned(),
            "skills/wechat-mp/bin/x".to_owned(),
        ];
        assert_eq!(
            detect("skills/wechat-mp", &files, &read_nothing),
            SkillLayout::Skill
        );
    }
}
