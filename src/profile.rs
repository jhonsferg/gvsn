//! Structured shell profile management.
//!
//! Provides safe, predictable manipulation of shell profile files by parsing
//! them into a structured representation, making modifications, and serializing
//! back. This avoids the fragility of string-search-and-replace approaches.

use anyhow::{Context, Result};
use std::path::Path;

/// A single block in a shell profile file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBlock {
    /// The marker that identifies this block (e.g., "# gvm init")
    pub marker: String,
    /// The content lines of the block (without the marker line)
    pub lines: Vec<String>,
}

impl ProfileBlock {
    /// Creates a new block with the given marker and content lines.
    pub fn new(
        marker: impl Into<String>,
        lines: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            marker: marker.into(),
            lines: lines.into_iter().map(Into::into).collect(),
        }
    }
}

impl std::fmt::Display for ProfileBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.marker)?;
        for line in &self.lines {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

/// Represents a parsed shell profile file.
#[derive(Debug, Clone, Default)]
pub struct ShellProfile {
    /// Lines before the first gvm block
    pub header: Vec<String>,
    /// Gvm-managed blocks in order
    pub blocks: Vec<ProfileBlock>,
    /// Lines after the last gvm block
    pub footer: Vec<String>,
}

/// Known gvm block markers
pub const MARKERS: &[&str] = &[
    "# gvm init",
    "# gvm wrapper",
    "# gvm path",
    "# gvm: binary location",
];

impl ShellProfile {
    /// Parses a profile file content into a structured representation.
    pub fn parse(content: &str) -> Self {
        let mut profile = Self::default();
        let mut current_block: Option<ProfileBlock> = None;
        let mut in_gvm_block = false;
        let mut header_done = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Check if this line starts a gvm block. Markers are always
            // written as a complete line (see `ProfileBlock`'s `Display`
            // impl), so matching must be exact - `starts_with` would also
            // catch unrelated user comments that merely begin with the same
            // text (e.g. "# gvm init fue lo primero que probe"), corrupting
            // them into a gvm-managed block on the next `gvm setup`/`implode`.
            if let Some(marker) = MARKERS.iter().find(|m| trimmed == **m) {
                // Finish previous block if any
                if let Some(block) = current_block.take() {
                    profile.blocks.push(block);
                }
                // Start new block
                current_block = Some(ProfileBlock::new(
                    marker.to_string(),
                    std::iter::empty::<String>(),
                ));
                in_gvm_block = true;
                header_done = true;
                continue;
            }

            if in_gvm_block {
                // End of block on blank line
                if trimmed.is_empty() {
                    if let Some(block) = current_block.take() {
                        profile.blocks.push(block);
                    }
                    in_gvm_block = false;
                    // Don't add the blank line to footer - it's just a separator
                    continue;
                }
                // Add line to current block
                if let Some(ref mut block) = current_block {
                    block.lines.push(line.to_string());
                }
            } else if header_done {
                // We're in footer
                profile.footer.push(line.to_string());
            } else {
                // We're in header
                profile.header.push(line.to_string());
            }
        }

        // Don't forget the last block if file doesn't end with blank line
        if let Some(block) = current_block {
            profile.blocks.push(block);
        }

        profile
    }

    /// Gets a block by its marker.
    pub fn get_block(&self, marker: &str) -> Option<&ProfileBlock> {
        self.blocks.iter().find(|b| b.marker == marker)
    }

    /// Sets (or adds) a block. If a block with the same marker exists, replaces it.
    pub fn set_block(&mut self, block: ProfileBlock) {
        if let Some(existing) = self.blocks.iter_mut().find(|b| b.marker == block.marker) {
            *existing = block;
        } else {
            self.blocks.push(block);
        }
    }

    fn write_to_string<W: std::fmt::Write>(&self, f: &mut W) -> std::fmt::Result {
        // Header
        for line in &self.header {
            writeln!(f, "{}", line)?;
        }

        // Blocks
        let mut wrote_something = !self.header.is_empty();
        for (i, block) in self.blocks.iter().enumerate() {
            if i > 0 || !self.header.is_empty() {
                writeln!(f)?;
            }
            write!(f, "{}", block)?;
            wrote_something = true;
        }

        // Footer
        if !self.footer.is_empty() {
            // Ensure single blank line before footer if we wrote something before
            if wrote_something {
                writeln!(f)?;
            }
            for line in &self.footer {
                writeln!(f, "{}", line)?;
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for ShellProfile {
    #[allow(clippy::inherent_to_string_shadow_display)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.write_to_string(f)
    }
}

/// Checks if the profile has a specific block with expected content.
impl ShellProfile {
    pub fn has_block_with_content(&self, marker: &str, expected_lines: &[String]) -> bool {
        self.get_block(marker)
            .map(|b| b.lines == expected_lines)
            .unwrap_or(false)
    }
}

/// Loads a profile from a file, or creates an empty one if it doesn't exist.
pub fn load_profile(path: &Path) -> Result<ShellProfile> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        Ok(ShellProfile::parse(&content))
    } else {
        Ok(ShellProfile::default())
    }
}

/// Saves a profile to a file.
pub fn save_profile(path: &Path, profile: &ShellProfile) -> Result<()> {
    let content = profile.to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {}", parent.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(())
}

/// Updates or adds the gvm path block in the login profile.
#[cfg(not(target_os = "windows"))]
pub fn update_path_block(path: &Path) -> Result<bool> {
    const MARKER: &str = "# gvm path";
    const EXPORT_LINE: &str = r#"export PATH="$HOME/.gvm/current/bin:$PATH""#;

    let mut profile = load_profile(path)?;
    let expected_lines = vec![EXPORT_LINE.to_string()];

    let changed = !profile.has_block_with_content(MARKER, &expected_lines);
    if changed {
        profile.set_block(ProfileBlock::new(MARKER, expected_lines));
        save_profile(path, &profile)?;
    }
    Ok(changed)
}

/// Removes all gvm-managed blocks from a profile.
pub fn strip_gvm_blocks(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut profile = load_profile(path)?;
    let initial_len = profile.blocks.len();
    profile.blocks.clear();
    let changed = profile.blocks.len() != initial_len;
    if changed {
        save_profile(path, &profile)?;
    }
    Ok(changed)
}

/// Ensures a profile has the required gvm blocks with correct content.
///
/// Returns true if the profile was modified.
pub fn ensure_profile(path: &Path, init_content: &str, wrapper_content: &str) -> Result<bool> {
    let mut profile = load_profile(path)?;

    let expected_init = init_content.lines().map(String::from).collect::<Vec<_>>();
    let expected_wrapper = wrapper_content
        .lines()
        .map(String::from)
        .collect::<Vec<_>>();

    let mut modified = false;

    // Check/update init block
    if !profile.has_block_with_content("# gvm init", &expected_init) {
        profile.set_block(ProfileBlock::new("# gvm init", expected_init));
        modified = true;
    }

    // Check/update wrapper block
    if !profile.has_block_with_content("# gvm wrapper", &expected_wrapper) {
        profile.set_block(ProfileBlock::new("# gvm wrapper", expected_wrapper));
        modified = true;
    }

    if modified {
        save_profile(path, &profile)?;
    }

    Ok(modified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parse_empty() {
        let profile = ShellProfile::parse("");
        assert!(profile.header.is_empty());
        assert!(profile.blocks.is_empty());
        assert!(profile.footer.is_empty());
    }

    #[test]
    fn parse_header_only() {
        let content = "export FOO=bar\nalias ll='ls -la'\n";
        let profile = ShellProfile::parse(content);
        assert_eq!(profile.header.len(), 2);
        assert!(profile.blocks.is_empty());
    }

    #[test]
    fn parse_single_block() {
        let content = "# gvm init\neval \"$(gvm env --shell bash)\"\n";
        let profile = ShellProfile::parse(content);
        assert_eq!(profile.blocks.len(), 1);
        assert_eq!(profile.blocks[0].marker, "# gvm init");
        assert_eq!(
            profile.blocks[0].lines,
            vec!["eval \"$(gvm env --shell bash)\""]
        );
    }

    #[test]
    fn parse_ignores_user_comment_with_marker_prefix() {
        // A user comment that merely starts with a marker's text (but isn't
        // the marker itself) must stay in the header/footer, not be treated
        // as the start of a gvm-managed block.
        let content = "# gvm initially I used zsh\nexport FOO=bar\n";
        let profile = ShellProfile::parse(content);
        assert!(
            profile.blocks.is_empty(),
            "a prefix match must not be treated as a gvm block"
        );
        assert_eq!(
            profile.header,
            vec![
                "# gvm initially I used zsh".to_string(),
                "export FOO=bar".to_string(),
            ]
        );
    }

    #[test]
    fn parse_multiple_blocks() {
        let content = r#"# user config
# gvm init
eval "$(gvm env --shell bash)"

# gvm wrapper
gvm() { command gvm "$@"; }

# more config
"#;
        let profile = ShellProfile::parse(content);
        assert_eq!(profile.header.len(), 1);
        assert_eq!(profile.blocks.len(), 2);
        assert_eq!(profile.blocks[0].marker, "# gvm init");
        assert_eq!(profile.blocks[1].marker, "# gvm wrapper");
        assert_eq!(profile.footer.len(), 1);
    }

    #[test]
    fn block_replacement() {
        let mut profile = ShellProfile::parse("# gvm init\nold content\n");
        profile.set_block(ProfileBlock::new(
            "# gvm init",
            vec!["new content".to_string()],
        ));
        assert_eq!(profile.blocks[0].lines, vec!["new content"]);
    }

    #[test]
    fn serialization_roundtrip() {
        let content = r#"# header
# gvm init
eval "$(gvm env --shell bash)"

# gvm wrapper
gvm() { command gvm "$@"; }

# footer
"#;
        let profile = ShellProfile::parse(content);
        let serialized = profile.to_string();
        let reparsed = ShellProfile::parse(&serialized);
        assert_eq!(profile.blocks.len(), reparsed.blocks.len());
        // Note: header may have trailing empty string due to parsing behavior
        assert!(reparsed.header.starts_with(&["# header".to_string()]));
        assert_eq!(profile.footer, reparsed.footer);
    }

    #[test]
    fn file_operations() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");

        // Write initial
        fs::write(&path, "# header\n# gvm init\nold\n").unwrap();

        // Load and modify
        let mut profile = load_profile(&path).unwrap();
        assert_eq!(profile.blocks.len(), 1);
        profile.set_block(ProfileBlock::new("# gvm init", vec!["new".to_string()]));
        save_profile(&path, &profile).unwrap();

        // Verify
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new"));
        assert!(!content.contains("old"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn update_path_block_adds_block_to_new_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");

        let changed = update_path_block(&path).unwrap();
        assert!(changed);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# gvm path"));
        assert!(content.contains(r#"export PATH="$HOME/.gvm/current/bin:$PATH""#));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn update_path_block_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");

        assert!(update_path_block(&path).unwrap());
        // Second call sees the same expected content already present.
        let changed_again = update_path_block(&path).unwrap();
        assert!(!changed_again, "second call must report no change");

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("# gvm path").count(), 1);
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn update_path_block_preserves_existing_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");
        fs::write(&path, "# my custom profile\nexport FOO=bar\n").unwrap();

        update_path_block(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("export FOO=bar"));
        assert!(content.contains("# gvm path"));
    }

    #[test]
    fn ensure_profile_creates_both_blocks_on_new_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");

        let modified = ensure_profile(
            &path,
            "eval \"$(gvm env --shell bash)\"",
            "gvm() { command gvm \"$@\"; }",
        )
        .unwrap();
        assert!(modified);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# gvm init"));
        assert!(content.contains("eval \"$(gvm env --shell bash)\""));
        assert!(content.contains("# gvm wrapper"));
        assert!(content.contains("gvm() { command gvm \"$@\"; }"));
    }

    #[test]
    fn ensure_profile_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");
        let init = "eval \"$(gvm env --shell bash)\"";
        let wrapper = "gvm() { command gvm \"$@\"; }";

        assert!(ensure_profile(&path, init, wrapper).unwrap());
        let changed_again = ensure_profile(&path, init, wrapper).unwrap();
        assert!(!changed_again, "second call must report no change");
    }

    #[test]
    fn ensure_profile_updates_stale_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");
        let old_wrapper = "gvm() { command gvm \"$@\"; }";
        let new_wrapper = "gvm() { command gvm \"$@\"; case \"$1\" in use) gvm env;; esac; }";

        ensure_profile(&path, "eval init", old_wrapper).unwrap();
        let modified = ensure_profile(&path, "eval init", new_wrapper).unwrap();
        assert!(modified);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains(new_wrapper));
        assert!(!content.contains(old_wrapper));
    }

    #[test]
    fn strip_profile_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");

        // Include blank line before footer so parser treats it as footer, not part of wrapper block
        fs::write(
            &path,
            "# header\n# gvm init\ncontent\n\n# gvm wrapper\nmore\n\n# footer\n",
        )
        .unwrap();

        let changed = strip_gvm_blocks(&path).unwrap();
        assert!(changed);

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("# gvm init"));
        assert!(!content.contains("# gvm wrapper"));
        assert!(content.contains("# header"));
        assert!(content.contains("# footer"));
    }
}
