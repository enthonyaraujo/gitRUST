# Internationalization

The typed message catalog in `crates/gitrust-ui/src/i18n.rs` pairs source English with Brazilian Portuguese. The locale resolver accepts `en-US` and `pt-BR` case-insensitively. Unknown locale identifiers use English. Settings changes the language at runtime and applies on the next frame. New sessions use English unless started with `--language pt-BR`.

All user-facing application and diagnostic control copy belongs in the catalog. Add a key and provide both translations for new controls. Preserve localized meaning in short labels and tests involving narrow panes. Label and menu responses recompute the display text from the active locale while their persistent widget IDs stay language-independent. Fallback lookup uses source English if a catalog entry lacks a Portuguese value.

Technical values such as Git file names, sample commit text, code, branches, hashes, diagnostic adapter names and the application version are displayed as supplied. Both shipped typefaces cover accented English and Portuguese text. At narrow widths the shell swaps list/detail panes and the settings dialog scrolls instead of relying on smaller fonts. See [ACCESSIBILITY.md](ACCESSIBILITY.md) for platform shortcuts.

File and commit counts use locale-specific singular and plural labels. Filtered lists show the matching and total counts. Gallery sizes use English or Brazilian Portuguese grouping, and diff line descriptions name the relevant before/after numbers in the active locale. Row identity uses the logical key and view scope across locale changes.
