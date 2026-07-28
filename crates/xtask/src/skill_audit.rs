use std::{fs, path::Path};

const SKILL_PATH: &str = ".agents/skills/boxology/SKILL.md";
const FRONTMATTER: &str = "---\nname: boxology\ndescription: Guide greenfield Boxology managed-project onboarding. Use when a developer asks a coding agent to initialize a Boxology-managed project; do not use for development of Boxology itself.\n---\n";
const HEADINGS: &[(u8, &str)] = &[
    (1, "Boxology onboarding"),
    (2, "Philosophy"),
    (2, "Box boundaries"),
    (2, "Contracts and compatible evolution"),
    (2, "Way of working"),
    (2, "Five-step onboarding flow"),
];
const BODIES: &[&str] = &[
    "Code is cheap; safe coordination and human attention are the bottleneck. Boxology makes the unit of change small enough for a coding agent to work on while keeping the decisions that shape a system legible to its human owner.\n\nHuman-owned boundaries, contracts, types, data models, and composition are the decisions that shape the system. The coding agent using this skill is the lead agent: it implements within those decisions and keeps communication on declared contracts. The implementation behind a contract is replaceable.",
    "A box is a managed package with one accountable owner. A box owns its implementation and declared boundary; a composition owns how boxes are wired and deployed; a platform package owns shared platform machinery. Every managed change has one accountable package and zero foreign source changes, except for deterministic artifacts attributable to the accountable package.\n\nKeep each boundary explicit. Do not reach into another package's implementation or create an undocumented communication path. Human-owned package boundaries and composition decisions are the guardrails that let the lead agent change one box without silently changing its neighbours.",
    "The authored controlled contract source is the source of truth for the public surface. Generated output is deterministic and checked in for review, but it is never hand-edited: change the authored source and regenerate it. The generated schema is the compatibility authority.\n\nPrefer an additive expansion, then migrate consumers, then contract the old surface: expand-migrate-contract. Preserve compatible evolution, and do not soften or relabel the generator's classifications for a tightening or removal merely because a migration is planned.",
    "The lead agent reads the repository instructions, README, and manifests before editing. It identifies the one accountable package, changes only its authored controlled source, regenerates deterministic outputs, runs the package's declared quality commands, and runs `boxology check`. It surfaces any protected control-plane change for human attention instead of treating that change as ordinary package work.",
];
const STEPS: &[&str] = &[
    "**Activate.** Apply this skill to the greenfield onboarding request; the coding agent becomes the lead agent for the new managed project.",
    "**Ask only.** Ask for the project name, target root, source checkout, and confirmation that the target is empty except `.git`.",
    "**Install both crates.** From the same source checkout, install both tools with the documented paths:\n`cargo install --path <source-checkout>/crates/boxology-init`\n`cargo install --path <source-checkout>/crates/boxology-cli`",
    "**Initialize explicitly.** Invoke `boxology-init` through its documented explicit interface with the answers from step 2. Consult that interface for the current flag spellings; this skill does not freeze flag spellings.",
    "**Build and check.** In the generated repository, run `cargo build` first so Cargo.lock is materialized, then run `boxology check`. The generated README owns the exact Rust and HTTP invocation detail.",
];
const INSTALLS: &[&str] = &[
    "`cargo install --path <source-checkout>/crates/boxology-init`",
    "`cargo install --path <source-checkout>/crates/boxology-cli`",
];
const FORBIDDEN: &[&str] = &[
    "host",
    "factory",
    "stage2",
    "post-v0",
    "github issues",
    "workers",
    "reviewers",
    "autonomous merging",
];

const PATH_CODE: &str = "BXSK001";
const FRONTMATTER_CODE: &str = "BXSK002";
const HEADING_CODE: &str = "BXSK003";
const SECTION_CODE: &str = "BXSK004";
const STEP_CODE: &str = "BXSK005";
const CONTENT_CODE: &str = "BXSK006";
const INSTALL_CODE: &str = "BXSK007";
const FORBIDDEN_CODE: &str = "BXSK008";

#[derive(Debug, Eq, PartialEq)]
struct Diagnostic {
    code: &'static str,
    message: String,
}

#[derive(Default)]
struct Section {
    level: u8,
    title: String,
    lines: Vec<String>,
}

pub(crate) fn run(root: &Path) -> u8 {
    let diagnostics = audit(root);
    if diagnostics.is_empty() {
        println!("skill-audit: PASS");
        0
    } else {
        eprintln!("skill-audit: FAIL");
        for diagnostic in diagnostics {
            eprintln!("{} {SKILL_PATH}: {}", diagnostic.code, diagnostic.message);
        }
        1
    }
}

fn audit(root: &Path) -> Vec<Diagnostic> {
    let mut directory = root.to_path_buf();
    for component in [".agents", "skills", "boxology"] {
        directory.push(component);
        match fs::symlink_metadata(&directory) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return vec![diag(
                    PATH_CODE,
                    format!("{component} must be a real directory"),
                )];
            }
            Ok(_) => {}
            Err(_) => {
                return vec![diag(PATH_CODE, format!("{component} directory is missing"))];
            }
        }
    }
    let path = directory.join("SKILL.md");
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return vec![diag(PATH_CODE, "skill path must not be a symlink")];
        }
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return vec![diag(PATH_CODE, "skill path is not a regular file")],
        Err(_) => return vec![diag(PATH_CODE, "skill file is missing")],
    }
    fs::read_to_string(path).map_or_else(
        |_| vec![diag(PATH_CODE, "skill file is not valid UTF-8")],
        |text| audit_text(&text),
    )
}

fn audit_text(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let body = match text.strip_prefix(FRONTMATTER) {
        Some(body) => body,
        None => {
            out.push(diag(
                FRONTMATTER_CODE,
                "frontmatter does not match the required name and trigger description",
            ));
            text
        }
    };
    let (visible, markup_bad) = visible_lines(body);
    if markup_bad {
        out.push(diag(
            HEADING_CODE,
            "Setext/HTML heading or fenced block is not allowed",
        ));
    }
    let sections = parse_sections(&visible);
    let first_visible = visible.iter().find(|line| !line.trim().is_empty());
    if first_visible.map(String::as_str) != Some("# Boxology onboarding")
        || sections.len() != HEADINGS.len()
        || sections
            .iter()
            .zip(HEADINGS)
            .any(|(got, expected)| (got.level, got.title.as_str()) != *expected)
    {
        out.push(diag(
            HEADING_CODE,
            "headings must exactly match the required ordered H1/H2 sequence",
        ));
    }
    if sections
        .first()
        .is_none_or(|section| !section_body(section).is_empty())
    {
        out.push(diag(
            SECTION_CODE,
            "H1 must not contain an extra section body",
        ));
    }
    for (index, expected) in BODIES.iter().enumerate() {
        if sections
            .get(index + 1)
            .is_none_or(|section| section_body(section) != *expected)
        {
            out.push(diag(
                SECTION_CODE,
                format!("section {} does not match its required body", index + 1),
            ));
        }
    }
    let steps = sections.get(5).map(parse_steps).unwrap_or_default();
    if steps.len() != STEPS.len()
        || steps
            .iter()
            .zip(STEPS)
            .enumerate()
            .any(|(index, ((number, body), expected))| *number != index + 1 || body != expected)
    {
        out.push(diag(
            STEP_CODE,
            "flow must contain the exact visible ordered steps 1-5",
        ));
    }
    let visible_text = visible.join("\n");
    if INSTALLS
        .iter()
        .any(|install| visible_text.match_indices(install).count() != 1)
    {
        out.push(diag(
            INSTALL_CODE,
            "both install commands must occur once and use <source-checkout>",
        ));
    }
    let final_step = steps.get(4).map(|(_, body)| body.as_str()).unwrap_or("");
    for command in ["`cargo build`", "`boxology check`"] {
        if final_step.match_indices(command).count() != 1 {
            out.push(diag(
                CONTENT_CODE,
                format!("{command} must occur exactly once"),
            ));
        }
    }
    let lower = visible_text.to_ascii_lowercase();
    for fragment in FORBIDDEN {
        if lower.contains(fragment) {
            out.push(diag(
                FORBIDDEN_CODE,
                format!("forbidden fragment {fragment:?} is present"),
            ));
        }
    }
    out
}

fn visible_lines(body: &str) -> (Vec<String>, bool) {
    let mut visible = Vec::new();
    let mut bad = false;
    for line in body.lines() {
        let trimmed = line.trim();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let setext = trimmed.len() >= 3
            && (trimmed.bytes().all(|byte| byte == b'=')
                || trimmed.bytes().all(|byte| byte == b'-'));
        let lower = trimmed.to_ascii_lowercase();
        let html = lower.contains("<!--")
            || lower.contains("<section")
            || (1..=6).any(|level| lower.contains(&format!("<h{level}")));
        bad |= fence || html || setext;
        visible.push(line.trim_end().to_owned());
    }
    (visible, bad)
}

fn parse_sections(lines: &[String]) -> Vec<Section> {
    let mut sections = Vec::<Section>::new();
    for line in lines {
        let level = line.bytes().take_while(|byte| *byte == b'#').count();
        let heading = (1..=6).contains(&level)
            && line
                .as_bytes()
                .get(level)
                .is_some_and(u8::is_ascii_whitespace);
        if heading {
            sections.push(Section {
                level: level as u8,
                title: line[level..].trim().to_owned(),
                lines: Vec::new(),
            });
        } else if let Some(section) = sections.last_mut() {
            section.lines.push(line.clone());
        }
    }
    sections
}

fn section_body(section: &Section) -> String {
    section.lines.join("\n").trim().to_owned()
}

fn parse_steps(section: &Section) -> Vec<(usize, String)> {
    let mut steps = Vec::<(usize, String)>::new();
    for line in &section.lines {
        let trimmed = line.trim();
        let item = line
            .split_once(". ")
            .and_then(|(number, body)| Some((number.parse().ok()?, body.to_owned())));
        if let Some(item) = item {
            steps.push(item);
        } else if !trimmed.is_empty()
            && let Some((_, body)) = steps.last_mut()
        {
            body.push('\n');
            body.push_str(trimmed);
        }
    }
    steps
}

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const VALID: &str = include_str!("../../../.agents/skills/boxology/SKILL.md");
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(std::path::PathBuf);

    impl TempRoot {
        fn bare() -> Self {
            let path = std::env::temp_dir().join(format!(
                "boxology-skill-audit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn new(text: Option<&str>) -> Self {
            let root = Self::bare();
            let path = root.0.join(SKILL_PATH);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if let Some(text) = text {
                fs::write(path, text).unwrap();
            }
            root
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn mutate(from: &str, to: &str) -> String {
        assert_eq!(VALID.match_indices(from).count(), 1, "{from:?}");
        VALID.replacen(from, to, 1)
    }

    fn rejects(text: &str, code: &str) {
        assert!(audit_text(text).iter().any(|error| error.code == code));
    }

    #[test]
    fn checked_in_passes_and_missing_or_symlink_paths_are_red() {
        assert!(audit(&crate::root()).is_empty());
        assert_eq!(run(&TempRoot::new(None).0), 1);
        #[cfg(unix)]
        {
            let root = TempRoot::new(None);
            let target = root.0.join("target");
            fs::write(&target, VALID).unwrap();
            std::os::unix::fs::symlink(target, root.0.join(SKILL_PATH)).unwrap();
            assert_eq!(
                audit(&root.0)[0].message,
                "skill path must not be a symlink"
            );

            let root = TempRoot::bare();
            let target = root.0.join("target");
            let skill = target.join("skills/boxology/SKILL.md");
            fs::create_dir_all(skill.parent().unwrap()).unwrap();
            fs::write(skill, VALID).unwrap();
            std::os::unix::fs::symlink(target, root.0.join(".agents")).unwrap();
            assert_eq!(
                audit(&root.0)[0].message,
                ".agents must be a real directory"
            );
        }
    }

    #[test]
    fn trigger_broadening_and_removal_are_red() {
        rejects(
            &mutate("Use when a developer asks", "Use whenever"),
            FRONTMATTER_CODE,
        );
        rejects(
            &mutate("; do not use for development of Boxology itself.", "."),
            FRONTMATTER_CODE,
        );
    }

    #[test]
    fn extra_atx_setext_and_html_headings_are_red() {
        for (level, title) in HEADINGS {
            rejects(
                &mutate(
                    &format!("{} {title}", "#".repeat(*level as usize)),
                    "## Removed",
                ),
                HEADING_CODE,
            );
        }
        rejects(
            &mutate("## Philosophy", "## Philosophy\n### Extra"),
            HEADING_CODE,
        );
        rejects(
            &mutate("## Philosophy", "## Philosophy\nHidden\n------"),
            HEADING_CODE,
        );
        rejects(
            &mutate("## Philosophy", "## Philosophy\n<h3>Hidden</h3>"),
            HEADING_CODE,
        );
        for prefix in ["    ", "\t"] {
            rejects(
                &mutate(
                    "# Boxology onboarding",
                    &format!("{prefix}# Boxology onboarding"),
                ),
                HEADING_CODE,
            );
        }
    }

    #[test]
    fn exact_sections_reject_hand_editing_reversal() {
        rejects(
            &mutate("it is never hand-edited", "it may be hand-edited"),
            SECTION_CODE,
        );
        for body in BODIES {
            rejects(&mutate(body, "Opposite policy."), SECTION_CODE);
        }
    }

    #[test]
    fn step_reorder_delete_add_indent_and_fenced_decoy_are_red() {
        rejects(&mutate("1. **Activate.**", "2. **Activate.**"), STEP_CODE);
        rejects(&mutate(STEPS[1], ""), STEP_CODE);
        rejects(
            &mutate(STEPS[4], &format!("6. Extra.\n{}", STEPS[4])),
            STEP_CODE,
        );
        let flow = VALID.split_once("1. **Activate.**").unwrap().1;
        let flow = format!("1. **Activate.**{}", flow.trim_end());
        rejects(&mutate(&flow, &format!("```\n{flow}\n```")), STEP_CODE);
        for prefix in ["    ", "\t"] {
            let indented = VALID
                .lines()
                .map(|line| {
                    if (1..=5).any(|number| line.starts_with(&format!("{number}. "))) {
                        format!("{prefix}{line}")
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            rejects(&indented, STEP_CODE);
        }
        let fenced = format!("{VALID}\n~~~\nAuToNoMoUs MeRgInG\n~~~\n");
        rejects(&fenced, HEADING_CODE);
        rejects(&fenced, FORBIDDEN_CODE);
    }

    #[test]
    fn installs_require_one_shared_checkout() {
        rejects(
            &mutate(
                "<source-checkout>/crates/boxology-cli",
                "<second-checkout>/crates/boxology-cli",
            ),
            INSTALL_CODE,
        );
        rejects(
            &mutate(INSTALLS[0], &format!("{}\n{}", INSTALLS[0], INSTALLS[0])),
            INSTALL_CODE,
        );
    }

    #[test]
    fn build_must_precede_check_in_the_fifth_step() {
        rejects(
            &mutate(
                "run `cargo build` first so Cargo.lock is materialized, then run `boxology check`",
                "run `boxology check` first, then run `cargo build` so Cargo.lock is materialized",
            ),
            STEP_CODE,
        );
    }

    #[test]
    fn every_forbidden_fragment_is_case_insensitive() {
        for fragment in FORBIDDEN {
            let mixed: String = fragment
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    if index.is_multiple_of(2) {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    }
                })
                .collect();
            rejects(&format!("{VALID}\n{mixed}"), FORBIDDEN_CODE);
        }
    }

    #[test]
    fn production_dispatch_and_ci_registration_fail_invalid_fixture() {
        let root = TempRoot::new(Some("invalid"));
        assert_eq!(crate::dispatch(&["skill-audit".to_owned()], &root.0), 1);
        assert!(!crate::registered_ci_skill_audits(&root.0));
        assert!(crate::registered_ci_skill_audits(&crate::root()));
    }
}
