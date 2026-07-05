//! UUID v4 generation, surfaced via the «🔮 UUID» menu.

use uuid::Uuid;

const MAX_UUIDS: u8 = 50;

/// Render `qty` uuids as MarkdownV2 text. Treats 0 as 1; caps at MAX_UUIDS.
pub fn render(qty: u8) -> String {
    let count = qty.clamp(1, MAX_UUIDS) as usize;
    (0..count)
        .map(|idx| format!("{}\\.  `{}`", idx + 1, Uuid::new_v4()))
        .collect::<Vec<_>>()
        .join("\n")
}
