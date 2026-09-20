# Backlog

Agents may propose entries here, but only Hervé writes them.

- Arrow-key and Esc support for the SPEC §9 console menu (decision.rs/fi's no-fzf branch). Today only digit-then-Enter selects, and Enter-on-anything-else cancels; there's no raw-keypress reader. Would need a small terminal-raw-mode dependency or a custom ReadKey loop in the pwsh script.