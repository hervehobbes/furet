# Backlog

## Remontées des agents

- Quand une requête Tab ne donne aucun résultat, pwsh propose à la place les fichiers et dossiers du répertoire courant. Ce comportement est peut-être voulu, peut-être pas.
- **`!alias/sub`** : `f !ombi/src` saute directement dans un sous-dossier de la cible d'un alias. Reporté à un lot dédié lors du cycle aliases (décision du 2026-10-02).
- **Scope par alias (`-in`)** : `f -in !ombi src` cherche `src` uniquement sous la cible de `!ombi`, sans y être ni que ce soit un dépôt git ; `-h` en deviendrait un cas particulier. Gardé pour plus tard (design §4).
- **Fuite de `complete` dans Tab** : `clap_complete` 4.6.11 ne filtre pas les sous-commandes cachées dans son script PowerShell, donc `furet alias <Tab>` propose `complete`. Limite acceptée le 2026-10-02 ; à revoir si clap corrige en amont.
- **Latence du hook `add`** : seul morceau de l'idée `furet stats` encore non livré (aucune mesure continue).
- **`furet queries` sans option** : afficher le journal des requêtes lui-même, pas seulement `--failures` (proposé au lot 79). Nouvelle exception stdout (décision d'Hervé), avec un format et une limite à définir comme pour `furet history`.

## idées
Hors ce qui est déjà dans les cartons (import historique PowerShell, `--history, boost contexte git, yazi, bash/zsh), voici ce que je vois, trié par rapport valeur/coût :

**Moyen coût, utile au quotidien**
**Touche au classement, donc ta décision**
**Pour la publication**
- Release GitHub CI + manifeste **scoop/winget** : sans ça, personne ne l'installera.

Mon top 3 : **stats avec latence du hook**, **mémoire des requêtes**. Laquelle veux-tu creuser en premier ?

- boost contexte git, yazi, bash/zsh
- **Contexte** : boost des dossiers du même dépôt git ou du même parent que le cwd.
- **Découverte** : scan optionnel de racines (`c:\dev` en profondeur 2, ou détection `.git` / `.sln` / `Cargo.toml`) pour que le premier saut marche sans visite préalable.
- **Mode interactif intégré** avec ratatui, sans dépendance à fzf, qui est pénible sous Windows. Pour le moteur, tu peux porter le tien (plus formateur) ou comparer avec `nucleo`, le matcher de Helix.

Comment remplacer z par f ? » est déjà couverte par `furet init pwsh --cmd z`.

- Arrow-key and Esc support for the SPEC §9 console menu (decision.rs/fi's no-fzf branch). Today only digit-then-Enter selects, and Enter-on-anything-else cancels; there's no raw-keypress reader. Would need a small terminal-raw-mode dependency or a custom ReadKey loop in the pwsh script.
