# Taste (Continuously Learned by [CommandCode][cmd])

[cmd]: https://commandcode.ai/

# design
- When presenting visual/design options, show concrete examples (ASCII art, mockups) before asking for a preference — abstract descriptions alone aren't enough for visual decisions. Confidence: 0.60
- For logo/wordmark designs in the TUI, prefer large ASCII art lettering (FIGlet-style character rendering of the name) over box-drawing icons, containers, or typographic wordmarks. Confidence: 0.65
- Avoid alternating row background colors (zebra striping) in TUI tables — it hurts readability for unselected rows. Keep unselected rows on a uniform background. Confidence: 0.80
- Use `Color::Rgb(0, 0, 0)` over `Color::Black` for foreground text that must be truly dark — ANSI Black gets remapped by some terminal themes. Confidence: 0.70

# tui
- Prompt for sudo password only when executing a privileged operation, not when entering the TUI. Notify the user that sudo is needed before confirming the operation. Confidence: 0.80
