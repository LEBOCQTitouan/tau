//! Termimad markdown rendering for `tau skill show --body`.
//!
//! Default-skin only in v1; user-customizable skins are a future
//! Skills-5 (or separate ROADMAP item) concern.

/// Render markdown text to ANSI-styled terminal output. Returns the
/// rendered string. Caller writes to stdout.
///
/// Width auto-detected via `termimad::terminal_size` (falls back to
/// 80 if no tty). Uses termimad's default `MadSkin`.
pub fn render_markdown(body: &str) -> String {
    let (cols, _rows) = termimad::terminal_size();
    let width = if cols > 0 { cols as usize } else { 80 };
    let skin = termimad::MadSkin::default();
    skin.text(body, Some(width)).to_string()
}
