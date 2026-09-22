# Backlog

- Idées de "marques/marqueurs", comme dans vim, pour naviguer d'un répertoire à un autre.

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



name: furet-sqlite-busy-timeout-backlog description: Open suggestion for furet — add busy_timeout/synchronous NORMAL pragmas to storage::configure(); omitted in lot 4 and still missing after lot 16 (2026-09-20) metadata: node_type: memory type: project originSessionId: sess_ba91f9ec-7bcf-4163-bc7e-7afd94528754
During lot 4 (SQLite schema, commit cbe0fcb, 2026-09-19), the storage layer sets WAL and foreign_keys but deliberately does NOT set busy_timeout or synchronous = NORMAL (both set by zoxide) because the lot prompt didn't request them.

Why: SPEC §5 enables WAL specifically because multiple terminals write concurrently; without busy_timeout, concurrent writes can fail immediately with SQLITE_BUSY.

How to apply: Lot 5 (commit 7e90bb6, 2026-09-19) added the write path (furet add upserts + inserts) but its prompt did not include the pragmas, so they are still missing. furet add now does two separate writes (INSERT OR IGNORE + SELECT, then INSERT) with no busy_timeout, so concurrent adds from several terminals can hit SQLITE_BUSY. Propose adding these pragmas to configure() in src/storage.rs — or propose a BACKLOG.md entry for Hervé (agents propose, never write BACKLOG.md themselves, per CLAUDE.md). Lot 12 (2026-09-20, commit 1c51931, top-level --help shows the db path) also touched storage usage without adding them. Lot 13 (2026-09-20, commit 9b9bb47) added the root DATABASE.md (docs-only, documenting storage.rs's MIGRATIONS; CLAUDE.md now has a "Database docs" rule requiring it to be updated with any schema change) — still no busy_timeout; the suggestion remains open and would now also touch DATABASE.md's pragma table. Lot 14 (2026-09-20, commits 727830a + 8d109aa, SPEC §17 file logging) added tracing instrumentation to the outer layers but still no pragmas; a side benefit is that SQLITE_BUSY failures now surface as ERROR lines in the <data dir>/logs/furet.<date>.log file, making concurrent-write incidents observable when this is finally proposed. Security audit of 2026-09-22 re-flagged the gap (finding "concurrency/robustness"): BACKLOG.md as of that date still has no busy_timeout entry.




## Propositions d'optimisations

Compris — j'élargis l'analyse à la base de données (schéma, index, pragmas, croissance). Je refais le benchmark en version compacte (création en un seul appel `mkdir`, 1 000 répertoires) pour ancrer les chiffres sans y passer une minute.

Les chemins `/tmp` POSIX ne sont pas résolus par le binaire Windows (piège déjà noté en mémoire). Je recommence avec des chemins Windows via `cygpath -w`.

Base peuplée (1 000 répertoires). Je mesure maintenant les quatre hot paths end-to-end, puis je vérifierai les plans de requête SQLite à plus grande échelle sur une copie de la base.

Chiffres de base obtenus — et confirmation en direct d'un bug déjà connu (`fi` appelle `query --list` sans `<QUERY>`, rejeté par clap : c'est l'entrée BACKLOG déjà proposée). Je décompose un peu plus (baseline sans base, requête stage-2 pure), puis je passe à la base à grande échelle.

~22 ms de base (spawn processus), ~17 ms pour lire + réconcilier 1 000 répertoires, ~10 ms de plus pour le classement. Maintenant l'angle base de données demandé : je gonfle une copie de la base à échelle réaliste (200 k visites ≈ un an d'usage intensif) et je mesure l'effet des index et des pragmas manquants.

Chiffres concluants — index manquants et pragma `synchronous` ont un effet mesuré massif à l'échelle. Je nettoie les répertoires de benchmark puis je rédige le rapport.

# Optimisations possibles — rapport

**Les gains les plus importants sont côté base de données, et ils sont maintenant mesurés** : deux index manquants divisent par 5 à 1 000 le coût des requêtes à l'échelle, et le pragma `synchronous` manque fait payer un `fsync` (~1,2 ms) à chaque `furet add` — c'est-à-dire à chaque changement de répertoire. Côté code, quatre corrections ciblées éliminent du travail redondant sur le chemin de requête.

## Mesures de référence

Mesuré sur ta machine avec un binaire release et une base de 1 000 répertoires (chaque chiffre = moyenne sur 20 exécutions) :

| Commande | Temps par appel | Au-dessus de la base |
|---|---|---|
| `furet home` (plancher : spawn + logging) | 22 ms | — |
| `furet query --list ""` (lecture + réconciliation) | 39 ms | +17 ms |
| `furet query prj500` (classement stage 1) | 48 ms | +26 ms |
| `furet query rpj5` (stage 2 sur les 1 000 candidats) | 49 ms | +27 ms |
| `furet add` (écriture) | 31 ms | +9 ms |

Le plancher de 22 ms est le coût du spawn de processus Windows — incompressible sans changer de design (daemon). Tout ce qui suit travaille sur la marge au-dessus.

Au passage, confirmé en direct : `furet query --list` sans `<QUERY>` est rejeté par clap, donc la branche fzf de `fi` (`pwsh.rs:107`) liste toujours vide. C'est le bug que je t'avais proposé pour le BACKLOG — il n'y figure toujours pas ; l'incident de mesure le re-confirme.

## 1. Base de données (les gains majeurs, mesurés à 200 k visites)

J'ai gonflé une copie de ta base à 200 020 visites / 50 040 requêtes journalisées (≈ un an d'usage intensif, une visite par changement de prompt) :

| Requête (appelée par) | Sans index | Avec index | Gain |
|---|---|---|---|
| `dir_entries` — **chaque `furet query`** | 61,1 ms | 11,9 ms | **5,1×** |
| `last_visited_dir` — chaque `f -` | 11,9 ms | 0,01 ms | **~1 000×** |
| `dir_listing` — `furet list` | 274 ms | 21,8 ms | **12,6×** |

**P1 — Deux index manquants** (migration v2). Le schéma n'a qu'un index (`idx_dirs_key`). `EXPLAIN QUERY PLAN` montre que `dir_entries` fait un `SCAN visits` complet + tri temporaire pour son `GROUP BY` ; à 200 k visites, cette requête seule coûte 61 ms sur **chaque** saut. Correctif :

```sql
CREATE INDEX idx_visits_dir_ts ON visits (dir_id, ts);
CREATE INDEX idx_visits_session_ts ON visits (session, ts);
```

Coût : 200 ms de création unique, +7 Mo sur une base de 8 Mo à cette échelle. Comme `visits` ne cesse de croître, sans index le temps de chaque requête croît linéairement avec ton historique — c'est le seul point qui se dégrade vraiment avec le temps. Ça se fait en lot migration (`MIGRATIONS` v2 + `user_version`, mise à jour de `DATABASE.md` comme l'exige CLAUDE.md).

**P2 — `synchronous = NORMAL`** (à adosser au point busy_timeout du BACKLOG). Mesuré : un commit d'une ligne passe de **1,16 ms à 0,03 ms** sous WAL. SQLite force le fsync à chaque commit en `FULL` par défaut, or `furet add` commit une fois par changement de répertoire — y compris le double write upsert+visite. C'est le même correctif que je t'avais proposé avec `busy_timeout` ; il a maintenant un argument performance mesuré en plus de l'aspect contention.

**P3 — `RETURNING id` dans `upsert_dir`** (`storage.rs:149-164`). Chaque `add` exécute un `INSERT ... ON CONFLICT` puis un `SELECT id` séparé ; SQLite ≥ 3.35 (tu as 3.53) permet `... DO UPDATE SET ... RETURNING id` — une instruction au lieu de deux, sur le chemin le plus fréquent de l'outil. Dans la boucle d'import, `prepare_cached` éviterait en outre de re-préparer les mêmes SQL 2N fois.

**P4 — Croissance non bornée**. `visits` et `queries` ne sont jamais purgées : ~200 k lignes/an en usage intensif, lues intégralement en mémoire par `queries --failures` (`calibration`). Une rétention (ex. visites > 12 mois) ou un plafond rendrait P1 moins critique encore, et recoupe la note vie privée de l'audit sécurité. Décision de design pour toi — je peux rédiger l'entrée BACKLOG.

## 2. Chemin de requête côté code

**P5 — Réconciliation O(N) `is_dir()` à chaque requête** (`main.rs:278-285`). Chaque `furet query` statifie *tous* les répertoires connus avant même de classer — à 1 000 répertoires ça représente l'essentiel des +17 ms de lecture, et c'est le pire cas sur lecteurs réseau (chaque `is_dir` = un aller-retour réseau). Options : réconcilier uniquement le top-K après classement, ne le faire que dans `add`, ou avec une période de grâce. C'est un changement de comportement SPEC §13 — ton choix.

**P6 — Double classement systématique** (`main.rs:437-445` puis `main.rs:322`). `resolve_pool` appelle `rank::rank` en entier juste pour tester la vacuité, puis `query_directories` reclasse tout. Faire retourner le classement par `resolve_pool` divise par deux le coût CPU de classement de chaque requête. Correctif trivial.

**P7 — Re-normalisation de la requête par candidat** (`stage1.rs:19`, `stage2.rs:60-67`). `stage1::score` redécoupe et re-normalise la requête (`Normalized::new` = 3 allocations de Vec) pour *chaque* candidat — c'est invariant de boucle. Pré-normaliser les tokens une fois par requête (API `score_tokens(&[Normalized], ...)`) et cacher la forme normalisée nom/dossier par candidat élimine la majorité des allocations du classement.

**P8 — Le tri recalcule sa clé à chaque comparaison** (`rank.rs:64`). `sort_by(|l, r| compare(&key(l), &key(r)))` reconstruit un `RankKey` par élément à chaque comparaison, dont un `name.chars().count()` O(longueur). Avec n log n comparaisons, c'est du travail redondant pur : précalculer les clés (decorate-sort) ou stocker `name_len` dans `Scored`.

**P9 — Micro-gains** (même lot si tu veux) : `same_path` et `list_by_recency` allouent deux `to_lowercase` par candidat (`rank.rs:121`, `main.rs:488`) — une clé précalculée + comparaison caractère à caractère suffit ; `paths::split` pourrait emprunter (`&str`) au lieu d'allouer deux `String` par candidat ; `stage1::explain` refait tout le travail de `score` une seconde fois (`stage1.rs:80`) — anecdotique car diagnostic ; le DP de stage 2 alloue trois Vec par fenêtre et pourrait s'arrêter dès que `best == 0`.

## Priorisation proposée

1. **P1 + P2** — un lot « base de données » (migration index + pragmas) : les plus gros gains, mesurés, et ils empêchent la dégradation avec le temps. P2 est déjà en attente de BACKLOG.
2. **P6 + P7 + P8** — un lot « moteur de classement » sans changement de comportement observable.
3. **P3, P9** — en rabiot dans les lots ci-dessus.
4. **P4, P5** — décisions de design (rétention, stratégie de réconciliation) : je te rédige les entrées BACKLOG si tu veux, la plume te reste.

Dis-moi par quoi tu veux commencer — les lots 1 et 2 sont prêts à être spécifiés en prompts de lot comme les précédents.











Je m'y mets : migration v2 (les deux index) + pragmas `synchronous = NORMAL` et `busy_timeout`. Je relis d'abord les conventions du projet (CLAUDE.md, DATABASE.md) pour respecter les règles de doc et de style.

Conventions bien notées (lot schéma isolé, DATABASE.md dans le lot, DoD obligatoire, JOURNAL.md par l'exécuteur). Je vérifie le format des entrées de journal et l'habitude de bump de version avant d'écrire le code.

Hervé a intégré l'audit sécurité dans BACKLOG.md entre-temps — arbre de travail propre, je peux committer sereinement. Je jette un œil au BACKLOG pour aligner mon lot avec ce qu'il a écrit.

Le BACKLOG intègre mes rapports — parfait, je n'y touche pas. Je regarde le format d'un prompt de lot récent, puis j'écris le code.

Format de prompt clair. Je lis MISTAKES.md (exigé par chaque lot) puis j'implémente.

Règle notée (un seul test mutant l'environnement — mes tests n'y touchent pas). J'implémente maintenant les changements dans `storage.rs`.

Maintenant les tests : mise à jour des trois assertions `user_version` existantes et ajout des deux nouveaux tests plus le helper.

Code en place. Mise à jour de DATABASE.md (règle CLAUDE.md : même lot que le changement de schéma).

J'écris le prompt d'archivage du lot et l'entrée de journal.

Tout est écrit. Lancement du DoD complet (`tools/Run-DoD.ps1` : fmt, clippy, tests, build, version).

Formatage à ajuster — j'applique `cargo fmt` et relance le DoD.

DoD vert. Je montre les nouveaux tests de migration spécifiquement, puis je commite le lot.

`prompts/` est ignoré par git (fichiers de lot locaux, comme les 26 précédents). Je vérifie et je commite sans ce répertoire.

Committed (`e35db55`). Je clôture ma note mémoire sur le busy_timeout (suggestion maintenant implémentée), puis le rapport.

Dernière étape de la convention de lot : la passe de review. Je l'exécute sur le diff du commit avant de clore.

# Lot 27 livré — index + pragmas (P1 + P2)

**Commit `e35db55`, DoD vert, review PASS.** Ta base existante migrera automatiquement vers `user_version = 2` au premier lancement du nouveau binaire (création des index en une transaction, ~200 ms à 200 k visites, bien moins à ta taille).

## Ce qui a changé

- **Migration 2** (`src/storage.rs`) : `idx_visits_dir_ts (dir_id, ts)` et `idx_visits_session_ts (session, ts)` — le script v1 est inchangé octet pour octet, seuls les nouveaux index s'ajoutent.
- **`configure()`** : `PRAGMA synchronous = NORMAL` (supprime le `fsync` par commit que chaque `furet add` payait) et `busy_timeout` 5 000 ms via l'API rusqlite (les writers concurrents attendent au lieu d'échouer en `SQLITE_BUSY`).
- **Tests** : 2 nouveaux (migration v1→v2 avec les deux index ; lecture des pragmas `synchronous=1`/`busy_timeout=5000`), 3 mis à jour (versions 1→2), aucun test mutant l'environnement — règle MISTAKES.md respectée.
- **Docs** : `DATABASE.md` (table pragmas + 2 lignes d'index), `JOURNAL.md` (entrée du lot), prompt archivé dans `prompts/lot-27-db-indexes-pragmas.md` (répertoire gitignored, comme les 26 autres).

## Preuve (sortie brute)

```
==> Step 4/5: cargo build --release
   Compiling furet v0.1.1 (C:\dev\furet)
    Finished `release` profile [optimized] target(s) in 40.11s
==> Step 5/5: run the release binary with --version
furet 0.1.1
All DoD steps passed.
```

```
test storage::tests::fresh_database_creates_all_tables_and_reaches_user_version_2 ... ok
test storage::tests::a_version_1_database_migrates_to_version_2_adding_the_visits_indexes ... ok
test storage::tests::opened_databases_run_synchronous_normal_with_a_busy_timeout ... ok
test storage::tests::migrating_an_already_migrated_database_is_a_noop ... ok
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 190 filtered out
```

Le reviewer confirme point par point : scope exact du prompt, script v1 identique, pas de requête production touchée, `DATABASE.md` cohérent avec le code, politique de commentaires conforme, JOURNAL ≤ 10 lignes.

## Suite possible

Le lot « moteur de classement » (P6 double classement, P7 re-normalisation, P8 tri) est le suivant par priorité — dis-moi si je le lance. La passe de review de ce lot étant faite, rien ne bloque le suivant.