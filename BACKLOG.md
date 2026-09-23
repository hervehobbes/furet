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


Les 6 lots sont faits : un commit chacun, DoD vert à chaque fois, tout est poussé sur origin/main (b18301e..9f5a099). Le nouveau binaire release est dans target\release\furet.exe.

┌─────┬─────────┬────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Lot │ Commit  │                                                          Contenu                                                           │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 28  │ 3e4915c │ furet query accepte maintenant d'être appelé sans requête : la liste initiale de fi avec fzf n'est plus vide               │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 29  │ 1087193 │ Un FURET_DATA_DIR vide est traité comme absent (F3)                                                                        │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 30  │ 6ba0bb2 │ Un seul classement par requête, et upsert_dir en une seule instruction (P6 + P3)                                           │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 31  │ b909deb │ Une requête ne vérifie sur le disque que les répertoires qui lui correspondent (P5)                                        │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 32  │ 1667e34 │ Migration 3 : index idx_visits_ts et idx_queries_ts, DATABASE.md à jour                                                    │
├─────┼─────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 33  │ 9f5a099 │ Clé retention_days (365 par défaut, 0 = tout garder) : chaque furet add supprime les visits et queries plus anciennes (P4) │
└─────┴─────────┴────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘

Mesures sur 1 000 répertoires (binaire release, moyenne de 30 lancements, trois tours en alternant ancien et nouveau binaire) :
- une requête avec un terme passe de 40–48 ms à 29 ms ;
- add reste à ~28 ms malgré la lecture de la config et la purge ;
- le simple lancement du processus coûte ~20 ms.

À savoir :
- Deux écarts avec SPEC.md, notés dans CONTRACTS.md :
  - Depuis P5, un répertoire disparu mais qui ne correspond à aucune requête garde son ancien statut. Il n'est marqué absent que quand une requête le fait remonter.
  - Depuis P4, l'historique des visites et des requêtes est limité à un an, alors que SPEC §5 prévoit un journal jamais purgé et §8 une fréquence enregistrée.
- furet add lit maintenant config.toml. Avant, il ne le lisait pas, pour rester rapide ;ue ça ne coûte rien.
- Le premier add avec le nouveau binaire supprime tout ce qui a plus d'un an. Si tu as importé des données zoxide plus anciennes, leurs répertoires restent connus mais perdent leur date de dernière visite. Ils sont alors départagés sur leur date de première apparition.
- Deux fichiers de tests existants ont été modifiés : tests/explain.rs fixe retention_days = 0 parce que ses visites de test datent de 2023, et le snapshot de l'aide de query a changé.
- La passe du sous-agent reviewer n'a pas été lancée (CLAUDE.md ne l'impose que pour lesque comme étape suivante ; dis-moi si tu la veux.

Le furet de ton PATH est C:\winutil\furet.exe, que je n'ai pas remplacé. Pour installer
! Copy-Item C:\dev\furet\target\release\furet.exe C:\winutil\furet.exe