const FILTER_NAVIGATION: (&str, &str) = (
    "PageUp/PageDown · Ctrl/Cmd+Home/End",
    "Move by visible rows / to first or last result (chapter, recent or comment filter)",
);

pub(super) const SHORTCUTS: &[(&str, &str)] = &[
    ("j / k / h / l", "Scroll down / up / left / right"),
    ("J / K   or   ] / [", "Next / previous page"),
    ("gg / G", "First / last page"),
    ("42G   or   42 Enter", "Go to page 42"),
    (
        "Backspace",
        "Remove the last count digit and cancel an unfinished g/z command",
    ),
    ("Ctrl+d / Ctrl+u", "Half screen down / up"),
    ("Space / Shift+Space", "Screen down / up"),
    (
        "50%   or   :50% Enter",
        "Jump to a percentage of the document's pages",
    ),
    (
        "zt / zz / zb",
        "Current page top / vertical center / bottom",
    ),
    (
        "zH / zL",
        "Scroll left / right half a screen; counts supported",
    ),
    (
        "z0 / z$",
        "Current page left / right edge; keep vertical position",
    ),
    (
        "3r / 3R",
        "Rotate clockwise / counterclockwise three quarter turns",
    ),
    (
        "Ctrl+e / Ctrl+y",
        "Scroll down / up without turning pages, optional count",
    ),
    ("Alt+Left / Alt+Right", "Jump history back / forward"),
    ("+ / −  ·  Ctrl/Cmd++ / −", "Zoom in / out"),
    ("Alt+= / Alt+−", "Increase / decrease PDF text weight"),
    (
        "3+ / 3−   or   4=",
        "Zoom by a count, preserving the viewport center",
    ),
    ("Alt + wheel / pinch", "Zoom at the pointer"),
    (
        "Ctrl/Cmd+Alt+Z / click zoom percentage",
        "Set an exact zoom percentage",
    ),
    ("p / w / e", "Fit page / width / height"),
    ("Shift+C", "Toggle continuous scrolling"),
    ("r / Shift+r", "Rotate clockwise / counterclockwise"),
    (
        "Mouse back / forward",
        "Restore the previous / next reading view",
    ),
    ("Alt+j / Alt+k", "Next / previous bookmark, optional count"),
    ("a / Ctrl/Cmd+0", "100% zoom, keeping the viewport center"),
    ("Ctrl/Cmd+p", "Find a recent document"),
    ("/   or   Ctrl/Cmd+f", "Search document"),
    (
        "n / N  ·  3n / 3N",
        "Next / previous search match, optional count",
    ),
    ("Alt+c / Alt+w", "Match case / whole words (search open)"),
    ("b", "Toggle bookmark"),
    ("Tab", "Toggle sidebar"),
    ("ma   /   'a or `a", "Save / restore named position a–z"),
    (
        "''   or   ``",
        "Toggle the last two jump positions, including the full view",
    ),
    ("t", "Toggle page previews and open sidebar"),
    (
        "c",
        "Find chapter by keywords · ↑/↓ or Ctrl+j/k · Enter to follow",
    ),
    FILTER_NAVIGATION,
    (
        "v, drag, then y",
        "Select and copy text (Ctrl/Cmd+C also works)",
    ),
    ("Ctrl/Cmd+a", "Select all text on the current page"),
    (
        "Double / triple click in v",
        "Select word / line; drag in those units, auto-scroll at edges",
    ),
    ("Ctrl/Cmd+r", "Reload PDF and keep reading position"),
    ("s, then y", "Document information, copy details"),
    ("Ctrl/Cmd+w", "Close PDF and save reading position"),
    (
        "Ctrl/Cmd+Shift+t",
        "Reopen the most recently closed document in this session",
    ),
    ("Ctrl/Cmd+q", "Quit and save the latest reading position"),
    ("d / i", "Toggle dark interface / invert page"),
    (
        "Ctrl/Cmd+Shift+h",
        "Add a highlight comment to selected text",
    ),
    (
        "Ctrl/Cmd+Enter",
        "Save the current comment directly in the PDF",
    ),
    (
        "Ctrl/Cmd+Shift+m",
        "Read PDF comments; select text and use Add comment to annotate",
    ),
    ("y", "Copy page text"),
    ("o   or   Ctrl/Cmd+o", "Open PDF"),
    (":", "Go to page dialog"),
    (
        ":+5 Enter / :-5 Enter",
        "Jump forward / back five physical pages; clamp at document edges",
    ),
    ("f, then a label", "Follow a visible PDF link"),
    ("F, then a label", "Copy a visible web-link address"),
    ("F11", "Toggle fullscreen"),
    ("? / F1 / Escape", "Find keyboard shortcuts / close dialog"),
    ("Right-click web link", "Copy link address"),
    (
        "Ctrl/Cmd+click PDF",
        "SyncTeX reverse search in the configured editor",
    ),
];

pub(super) const FIXED_SHORTCUTS: &[(&str, &str)] = &[
    FILTER_NAVIGATION,
    ("gg / 42gg / 42 Enter", "First page / counted page"),
    ("Backspace", "Edit count and cancel unfinished g/z command"),
    (
        "50%   or   :50% Enter",
        "Jump to a percentage of the document's pages",
    ),
    ("zt / zz / zb", "Align current page top / center / bottom"),
    (
        "zH / zL / z0 / z$",
        "Scroll half a screen / align horizontal edges",
    ),
    ("Alt + wheel / pinch", "Zoom at the pointer"),
    ("Click zoom percentage", "Set an exact zoom percentage"),
    (
        "Mouse back / forward",
        "Restore previous / next reading view",
    ),
    ("ma / 'a / `a", "Save / restore named position a–z"),
    ("'' / ``", "Toggle the last two jump positions"),
    ("Ctrl/Cmd+a / Ctrl/Cmd+c", "Select all / copy selected text"),
    (
        "Double / triple click in selection mode",
        "Select word / line",
    ),
    ("Ctrl/Cmd+Enter", "Save current comment"),
    ("Alt+c / Alt+w", "Match case / whole words (search open)"),
    ("Escape", "Close dialog / cancel current mode"),
    ("Right-click web link", "Copy link address"),
    ("Ctrl/Cmd+click PDF", "SyncTeX reverse search"),
];
