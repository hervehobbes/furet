# Backlog

- Idées de "marques/marqueurs", comme dans vim, pour nabiguer d'un répertoire à un autre

**2. `furet remove <path>`**
C'est l'équivalent de `zoxide remove` : oublier un répertoire indésirable. Point de conception à trancher : soit tu réutilises `missing_since`, mais ça mélange la sémantique avec le soft delete, soit tu ajoutes une colonne `removed_at`. Dans ce second cas, c'est un lot schéma à part, avec la mise à jour de `DATABASE.md`.

**3. Clé `exclude_dirs` dans `config.toml`**
Des globs de répertoires jamais enregistrés par `add`, comme `_ZO_EXCLUDE_DIRS` dans zoxide. C'est de la config pure, sans schéma. Petit piège : `add` ne lit pas la config aujourd'hui (« add stays cheap »), donc il faudra décider si ce coût est acceptable.

Comment remplacer z par f ? » est déjà couverte par `furet init pwsh --cmd z`.

Vérifie dans `SPEC.md` si la complétion ou `remove` y figurent. Sinon, c'est une extension de périmètre
Agents may propose entries here, but only Hervé writes them.

- Arrow-key and Esc support for the SPEC §9 console menu (decision.rs/fi's no-fzf branch). Today only digit-then-Enter selects, and Enter-on-anything-else cancels; there's no raw-keypress reader. Would need a small terminal-raw-mode dependency or a custom ReadKey loop in the pwsh script.

## Audit de sécurité

Je vais commencer par explorer la structure du projet pour comprendre ce qu'est Furet et son ampleur, puis je mènerai l'audit de sécurité systématiquement.

C'est un CLI Rust (un « zoxide-like » pour Windows) — 18 fichiers sources. Je vais mesurer l'ampleur puis lire le code sensible en priorité : génération de shell (pwsh), stockage SQLite, gestion des chemins, import de données externes.

Environ 6 200 lignes de Rust — je peux tout auditer moi-même. Je lis d'abord les fichiers les plus sensibles : génération PowerShell (risque d'injection), import de données externes, chemins, logging.

Les premiers fichiers révèlent des points à vérifier (interpolation de `cmd` dans le script PowerShell, échappement des complétions). Je lis maintenant le stockage SQLite et le point d'entrée CLI.

Le stockage est propre : toutes les requêtes sont paramétrées (`params![]`). Je passe au point d'entrée CLI et à la configuration (notamment la validation du nom de fonction injecté dans le script PowerShell).

`main.rs` confirme un point d'attention : `--cmd` de `furet init pwsh` est interpolé sans validation dans le script généré. Je lis maintenant les modules restants (fallback disque, soft-delete, horloge, parsing/ranking) pour compléter la couverture.

Modules disque/soft-delete/horloge : rien de critique. Je termine la lecture du code avec les modules de parsing et de classement (recherche de paniques/DoS sur entrées utilisateur), puis je vérifierai les dépendances et les scripts outils.

La logique pure (normalize/stage1/stage2/decision) est défensive : arithmétique saturée, index vérifiés. Je passe aux vérifications transversales : dépendances (Cargo.lock, CVE connues), scripts PowerShell des outils, et recherche de motifs risqués dans le code restant.

Aucun spawn de processus depuis le Rust (fzf est lancé par le script PowerShell généré, pas par furet) — bon point. Je vérifie maintenant les versions de dépendances dans Cargo.lock, notamment la version de SQLite embarquée par rusqlite (source de CVE récurrentes).

Arbre de dépendances mince, sans crate réseau — bon. Je vérifie la version de SQLite embarquée et les avis de sécurité récents (RustSec) sur ces crates.

SQLite embarqué 3.53.2 (récent, sans CVE ouverte connue) et tracing-subscriber 0.3.23 ≥ 0.3.20 (correctif RUSTSEC-2025-0055 inclus) — dépendances propres. Je termine avec les hooks/outils, les derniers modules, et l'historique git.

Dernières vérifications : le comportement de fzf sur Windows avec le placeholder `{q}` (injection potentielle), le BACKLOG, et les instructions d'installation du README.

L'audit est complet. Je mets à jour ma note mémoire sur le busy_timeout (toujours absent du BACKLOG actuel, confirmé par cet audit) puis je rédige le rapport final.

# Audit de sécurité — Furet 0.1.1

**Verdict : aucune vulnérabilité critique.** L'outil est local (aucune crate réseau, aucun serveur), le SQL est intégralement paramétré, et le binaire ne lance aucun processus lui-même. J'ai relevé **2 points de durcissement worth fixing** (injection dans le script généré, placeholder fzf), **1 faille de robustesse** (variable d'environnement vide) et quelques notes informatives.

---

## Périmètre et méthode

Lecture intégrale des 18 fichiers Rust (~6 200 lignes), du script PowerShell généré (`src/pwsh.rs`), de `Cargo.lock`, des scripts `tools/`, des hooks git, et de l'historique git (recherche de secrets). Vérification des dépendances contre les avis RustSec.

## Ce qui est solide

- **Zéro injection SQL** : toutes les requêtes `rusqlite` utilisent des statements paramétrés (`params![]`) — la seule concaténation SQL du projet est un helper de test sur un nom de table fixe (`storage.rs:902`).
- **Aucun spawn de processus depuis le Rust** : fzf est invoqué par le script PowerShell généré, jamais par furet. Pas de surface d'injection de commandes côté binaire.
- **Tout chemin stocké est canonicalisé à l'entrée** (`dunce::canonicalize`, jonctions et symlinks résolus) via `paths::canonical`, aussi pour l'import zoxide — un chemin inexistant ou fichier est rejeté. Côté shell, tous les `cd` passent par `Set-Location -LiteralPath`, et les chemins sont passés après `--` dans les appels `furet add`/`query`.
- **Le compléteur Tab échappe correctement** les apostrophes PowerShell (`pwsh.rs:166-171`) ; le menu sans fzf valide la saisie par regex.
- **Moteur de scoring défensif** : arithmétique saturée/`checked_sub` partout, pas d'indexation non gardée, et `clippy::unwrap_used/expect_used = deny` en CI locale (`Run-DoD.ps1`).
- **Base de données** : contraintes `CHECK` sur `source`/`stage`, clés étrangères activées, WAL.
- **Dépendances propres** : pas de crate réseau ; SQLite embarqué **3.53.2** (récent, pas de CVE ouverte connue) ; `tracing-subscriber 0.3.23` est ≥ 0.3.20 donc **patché pour RUSTSEC-2025-0055** (injection ANSI) — et furet écrit de toute façon ses logs dans un fichier avec `with_ansi(false)`.

## Constats

### F1 (Moyen — durcissement) : `--cmd` injecté sans validation dans le script PowerShell

`furet init pwsh --cmd <X>` interpole `X` tel quel dans le gabarit via `TEMPLATE.replace(...)` (`pwsh.rs:183`, appelé depuis `main.rs:675`). Un nom de fonction malveillant comme `f { calc }; function global:f` produirait un script de profil contenant du code arbitraire — et le README recommande précisément `Invoke-Expression (& furet init pwsh | Out-String)`, donc ce qui sort de cette commande s'exécute. Le scénario d'attaque demande qu'un attaquant contrôle l'argument (ingénierie sociale / script d'installation compromis), d'où « durcissement » plutôt que « critique », mais la parade coûte cinq lignes : **valider `cmd` contre `^[A-Za-z][A-Za-z0-9_-]*$` et refuser sinon**.

### F2 (Faible) : placeholder `{q}` de fzf exécuté par cmd.exe

Dans `fi` (`pwsh.rs:108`), le bind `change:reload:furet query --list --color {q}` est exécuté par fzf via `cmd.exe /c` sur Windows (sauf `SHELL`/`--with-shell` configurés). fzf entoure `{q}` d'apostrophes simples, ce qui protège sous `sh` mais **ne protige rien sous cmd.exe** : taper `& calc` dans le prompt fzf de `fi` exécuterait `calc`. La saisie venant de l'utilisateur lui-même, c'est surtout auto-infligé — mais c'est aussi un bug fonctionnel (toute requête contenant `&`, `|`, `>` casse le rechargement). Pistes : filtrer les métacaractères, documenter l'exigence `--with-shell`, ou régénérer la liste sans `{q}`.

### F3 (Faible — robustesse) : `FURET_DATA_DIR` vide accepté

`data_dir()` (`storage.rs:65`) transforme une variable vide en `PathBuf::from("")`, donc une base **relative au répertoire courant**. Si cette variable vide traîne dans l'environnement, le hook de prompt `furet add` crée/lit un `furet.db` local dans chaque répertoire visité — un répertoire hostile pourrait embarquer une base pré-fabriquée qui influence les sauts. Reco : traiter une valeur vide comme absente (`if !dir.is_empty()`).

### Notes informatives

- **`import zoxide` lit stdin sans limite** (`read_to_end`, `main.rs:597`) — épuisement mémoire possible avec un stdin hostile ; impact faible pour un outil local.
- **Pas de `busy_timeout` SQLite** : le hook de prompt écrit à chaque affichage de prompt ; plusieurs terminaux ouverts peuvent se heurter à des erreurs `SQLITE_BUSY` transitoires. C'est le point que je t'avais déjà signalé — il n'apparaît toujours pas dans BACKLOG.md, et cet audit le re-confirme comme entrée à proposer.
- **Vie privée** : la base et 7 jours de logs conservent l'historique complet des répertoires visités (comme zoxide ou l'historique bash). Les ACL de `%LOCALAPPDATA%` limitent l'accès au compte utilisateur — acceptable, mais ça mérite une phrase dans le README.
- **Noms de répertoires exotiques** : un nom contenant un saut de ligne ou une séquence ANSI (créable via `\\?\` ou WSL) fausserait la sortie ligne-par-ligne de `--list`/fzf ou manipulerait l'affichage du terminal. `Set-Location -LiteralPath` reste sûr ; c'est du bruit d'affichage au pire.
- **Échec permissif des exclusions** : une entrée `exclude` de config invalide fait marcher le fallback *sans aucune exclusion* (`fallback.rs:113`) — impact performance seulement.
- **Pas de CI distante** (pas de `.github/`) : le DoD repose sur `tools/Run-DoD.ps1` ; pas de workflow à auditer, mais aussi pas de garde-fou automatisé contre une régression des points ci-dessus.

## Recommandations par priorité

1. **Valider `--cmd`** dans `init pwsh` (F1) — petite fonction + test dans `tests/pwsh.rs`.
2. **Ignorer `FURET_DATA_DIR` vide** (F3) — one-liner dans `storage.rs`.
3. **Atténuer ou documenter `{q}`/fzf** (F2).
4. **Ajouter `busy_timeout` (et `synchronous = NORMAL`) au BACKLOG** — je te propose l'entrée, tu restes seul rédacteur du BACKLOG selon CLAUDE.md.

Sources : [RUSTSEC-2025-0055](https://rustsec.org/advisories/RUSTSEC-2025-0055.html), [base d'avis RustSec](https://github.com/rustsec/advisory-db), [man fzf](https://www.mankier.com), [fzf ADVANCED.md](https://sourcegraph.com).
