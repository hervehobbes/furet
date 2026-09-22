# Backlog

- Comment remplacer z par f ?

**1. Complétion Tab dans pwsh** (mon choix)
`f mcp<Tab>` fait défiler les candidats classés, via `Register-ArgumentCompleter` qui appelle `furet query <texte> --list`. zoxide fait la même chose (`z foo<Space><Tab>`), donc tu as une référence à citer. Le lot touche uniquement `src/pwsh.rs` et `tests/pwsh.rs`, sans changement de schéma ni du moteur. Ça comble aussi une partie du besoin de ta proposition « flèches/Esc » du BACKLOG, sans dépendance terminal.

**2. `furet remove <path>`**
C'est l'équivalent de `zoxide remove` : oublier un répertoire indésirable. Point de conception à trancher : soit tu réutilises `missing_since`, mais ça mélange la sémantique avec le soft delete, soit tu ajoutes une colonne `removed_at`. Dans ce second cas, c'est un lot schéma à part, avec la mise à jour de `DATABASE.md`.

**3. Clé `exclude_dirs` dans `config.toml`**
Des globs de répertoires jamais enregistrés par `add`, comme `_ZO_EXCLUDE_DIRS` dans zoxide. C'est de la config pure, sans schéma. Petit piège : `add` ne lit pas la config aujourd'hui (« add stays cheap »), donc il faudra décider si ce coût est acceptable.

**4. `furet list`**
Un dump des répertoires connus avec leur nombre de visites et leur dernière visite, pratique pour déboguer la base. Il faut respecter la discipline stdout, comme l'exception déjà acceptée pour `queries --failures`.

Au passage, l'entrée du BACKLOG « Comment remplacer z par f ? » est déjà couverte par `furet init pwsh --cmd z`.

Vérifie dans `SPEC.md` si la complétion ou `remove` y figurent. Sinon, c'est une extension de périmètre, donc ta décision. Tu veux que je rédige le prompt du lot 25 pour la complétion ?

Agents may propose entries here, but only Hervé writes them.

- Arrow-key and Esc support for the SPEC §9 console menu (decision.rs/fi's no-fzf branch). Today only digit-then-Enter selects, and Enter-on-anything-else cancels; there's no raw-keypress reader. Would need a small terminal-raw-mode dependency or a custom ReadKey loop in the pwsh script.