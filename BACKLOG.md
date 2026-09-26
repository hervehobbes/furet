# Backlog

## idées
Hors ce qui est déjà dans les cartons (import historique PowerShell, `--history`, alias `@`, nucleo, marques, boost contexte git, yazi, bash/zsh), voici ce que je vois, trié par rapport valeur/coût :

**Petit coût, gain immédiat**
- **Aperçu dans `fi`** : `--preview` fzf qui liste le contenu du dossier survolé. Une ligne dans le script pwsh.
- **Complétions des sous-commandes** via `clap_complete` (`furet remove --<Tab>`…). Mécanique, idéal pour GLM Flash.
- **`furet remove --missing`** : purger d'un coup les dossiers disparus depuis longtemps. Réutilise `remove_dirs`.
- **Lecteurs lents ou débranchés** : un `stat` sur un chemin UNC ou un disque réseau hors ligne peut geler la réconciliation. Un timeout ou une exclusion des lecteurs réseau évite un `f` qui fige 30 s.

**Moyen coût, utile au quotidien**
- **`furet stats`** : top dossiers, taux d'échecs (§15), et surtout **latence du hook `add`**. C'était le risque n°1 identifié sous Windows, et tu n'as aucune mesure en continu.
- **Requête limitée au projet courant** : `f -l src` ne cherche que sous la racine git du cwd. Évite le `src` d'un autre dépôt, sans toucher au classement global.
- **Export / import JSON** pour sauvegarder ou passer d'une machine à l'autre (pendant simple de la synchro atuin).

**Touche au classement, donc ta décision**
- **Mémoire des requêtes** : si `f cl` t'a déjà mené à `ombi` sans échec derrière, ce couple gagne la fois suivante. C'est la vraie réponse à ton exemple D1 `c:\cl` vs `c:\dev\ombi`, et le journal `queries` contient déjà les données. Mais c'est un nouveau critère de tri, pas de la récence : à trancher avant tout lot.

**Pour la publication**
- Release GitHub CI + manifeste **scoop/winget** : sans ça, personne ne l'installera.

Mon top 3 : **stats avec latence du hook**, **mémoire des requêtes**, **aperçu fzf**. Laquelle veux-tu creuser en premier ?

- import historique PowerShell, --history, alias @, nucleo, marques, boost contexte git, yazi, bash/zsh
- **Contexte** : boost des dossiers du même dépôt git ou du même parent que le cwd.
- **Découverte** : scan optionnel de racines (`c:\dev` en profondeur 2, ou détection `.git` / `.sln` / `Cargo.toml`) pour que le premier saut marche sans visite préalable.
- **Épingles / alias** : `f @ombi` pointe vers un chemin fixe.
- **Historique de session** : `f -`, `f --back 3`.
- **Mode interactif intégré** avec ratatui, sans dépendance à fzf, qui est pénible sous Windows. Pour le moteur, tu peux porter le tien (plus formateur) ou comparer avec `nucleo`, le matcher de Helix.

Comment remplacer z par f ? » est déjà couverte par `furet init pwsh --cmd z`.

Agents may propose entries here, but only Hervé writes them.

- Idées de "marques/marqueurs", comme dans vim, pour naviguer d'un répertoire à un autre.

- Arrow-key and Esc support for the SPEC §9 console menu (decision.rs/fi's no-fzf branch). Today only digit-then-Enter selects, and Enter-on-anything-else cancels; there's no raw-keypress reader. Would need a small terminal-raw-mode dependency or a custom ReadKey loop in the pwsh script.
