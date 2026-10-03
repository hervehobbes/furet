# Exemples d'utilisation de furet

Ces exemples supposent que :
- l'intégration PowerShell est chargée (`Invoke-Expression (& furet init pwsh | Out-String)` dans le `$PROFILE`), ce qui donne les fonctions `f` et `fi` ;
- vous partez, par défaut, du répertoire `C:\dev\furet` ;
- l'arborescence sous `C:\dev` est celle-ci (extrait) :

```
C:\dev
├── clypher
│   ├── Clypher
│   ├── Clypher.Tests
│   └── Clypher.UiTests
├── CodeGroups
│   ├── CodeGroups.Mcp
│   ├── CodeGroups.Mcp.Tests
│   ├── CodeGroups.Shared
│   └── CodeGroups.Tests
├── formation_dotnet
│   ├── BanqueDLL
│   ├── BanqueDllTests
│   └── formation_dotnet
├── furet
│   ├── prompts
│   ├── src
│   ├── target
│   ├── tests
│   └── tools
├── image_docker_light
│   └── src
├── RedditForKarakeep
│   ├── api
│   ├── cli
│   ├── mcp
│   ├── observability
│   ├── proxy
│   ├── tui
│   └── ui
├── sourcier
│   ├── outils
│   ├── reference
│   ├── spike
│   ├── src
│   └── tests
└── sourcier-mesures
```

furet n'apprend que les répertoires que vous avez **déjà visités** : plus
vous vous en servez, plus les sauts deviennent précis. Les exemples
ci-dessous supposent que vous avez, à un moment ou un autre, déjà mis les
pieds dans les dossiers cités (le hook `f` enregistre chaque `cd`).

## Se déplacer avec `f`

### Saut par fragment flou

```powershell
PS C:\dev\furet> f clypher
PS C:\dev\clypher>
```

`f` prend un fragment flou du chemin, pas un chemin complet. Il peut
matcher n'importe quel segment du chemin, pas seulement le dernier :

```powershell
PS C:\dev\furet> f mcp
PS C:\dev\CodeGroups\CodeGroups.Mcp>
```

Si plusieurs répertoires visités correspondent à `mcp` (ici
`CodeGroups.Mcp`, `CodeGroups.Mcp.Tests`, `RedditForKarakeep\mcp`), furet
choisit le mieux classé selon son moteur de scoring ; en cas d'égalité de
score, le plus récemment visité l'emporte (mais un score moins bon ne
l'emporte jamais sur la récence — voir la règle D1 de `CLAUDE.md`).

### Fragments multiples

Vous pouvez enchaîner plusieurs fragments pour désambiguïser :

```powershell
PS C:\dev\furet> f reddit ui
PS C:\dev\RedditForKarakeep\ui>
```

### Complétion avec Tab

`Tab` complète le premier argument avec les répertoires que furet classe
pour le fragment tapé, dans l'ordre exact de `furet query --list` :

```powershell
PS C:\dev\furet> f mcp<Tab>   # propose CodeGroups.Mcp, mcp, mcp.Tests...
PS C:\dev\furet> f C:\dev\CodeGroups\CodeGroups.Mcp
```

Accepter une proposition insère le chemin complet ; `Entrée` saute ensuite
par la branche chemin direct de `f`. Sans fragment (`f <Tab>`), la liste
complète est proposée. La complétion ne s'applique qu'au premier argument :
ni `-`/`--explain`, ni `.`/`..`/`...`, ni un deuxième fragment.

### Rester dans le projet courant

`f -l` restreint la recherche au **projet git courant** : la racine du
projet est le plus proche ancêtre du répertoire courant qui contient une
entrée `.git` — un répertoire **ou un fichier** (un worktree ou un
submodule compte donc aussi). Les répertoires connus situés hors de cette
racine sont ignorés, même mieux classés, et le fallback disque ne remonte
jamais au-dessus d'elle :

```powershell
PS C:\dev\furet\src> f -l mcp
PS C:\dev\furet\...>        # jamais en dehors de C:\dev\furet
```

Appelé sans argument, `f -l` saute directement à la racine du projet :

```powershell
PS C:\dev\furet\src> f -l
PS C:\dev\furet>
```

Hors d'un dépôt git, `f -l` échoue (`furet: not inside a git repository`)
et ne déplace rien. `f -l <Tab>` ne propose que des répertoires du projet.
`fi -l` applique la même restriction au choix interactif (liste initiale et
rechargements de `fzf` compris), et `f -l mcp --explain` affiche le rapport
de score de la requête restreinte, avec une ligne `project root:` nommant
la racine. Côté binaire, le drapeau s'appelle `--local` et se combine avec
les autres : `furet query --local --list`, `furet query --list --color
--local`, etc.

### Rester sous la maison

`f -h` applique la même restriction, mais autour de la **maison** : la clé
`home` de `config.toml` (validée comme par `furet home`), sinon `$HOME`.
Contrairement à `-l`, la portée ne dépend pas du répertoire courant : `f -h`
fonctionne de n'importe où, y compris depuis l'extérieur de la maison.
Avec `home = "C:\\dev"` :

```powershell
PS C:\Users\thouz> f -h mcp
PS C:\dev\CodeGroups\CodeGroups.Mcp>

PS C:\Users\thouz> f -h
PS C:\dev>
```

Le repli disque suit la portée : depuis un répertoire **dans** la maison,
il part du répertoire courant et ne remonte jamais au-dessus de la maison ;
depuis l'extérieur, il parcourt les enfants de la maison elle-même, sans
monter. `fi -h` restreint le choix interactif, `f -h mcp<Tab>` ne propose
que des répertoires sous la maison, et `f -h mcp --explain` affiche une
ligne `home root: C:\dev` là où `-l` affiche `project root:`. Combiner les
deux (`f -l -h mcp`) est refusé : le shell affiche l'erreur de `furet
query` (`cannot be used with`) et ne bouge pas. Côté binaire, le drapeau
s'appelle `--home` : `furet query --home mcp --list`, etc.

### Chemin direct

Si l'argument est un chemin qui existe tel quel, `f` y saute directement
sans passer par le classement :

```powershell
PS C:\dev\furet> f C:\dev\sourcier\spike\jalon1-enum
PS C:\dev\sourcier\spike\jalon1-enum>
```

### Remonter dans l'arborescence

```powershell
PS C:\dev\furet\src\snapshots> f ..
PS C:\dev\furet\src>

PS C:\dev\furet\src\snapshots> f ...
PS C:\dev\furet>
```

### Revenir en arrière

`f -` revient au répertoire précédent **de la session de terminal en
cours** :

```powershell
PS C:\dev\furet> f clypher
PS C:\dev\clypher> f -
PS C:\dev\furet>
```

### Retour à la maison

```powershell
PS C:\dev\clypher\Clypher.Tests> f
PS C:\Users\thouz>
```

La cible peut être personnalisée avec la clé `home` de `config.toml` (chemin
absolu, `/` accepté) ; sans elle, `f` sans argument va vers `$HOME`. Sans
argument, `f -h` mène au même endroit, via la requête restreinte à la
maison plutôt que par `furet home`.

### Ne jamais enregistrer certains répertoires

Le hook enregistre chaque `cd` — y compris dans des répertoires qu'on ne
veut pas voir remonter dans les classements. La clé `exclude_dirs` de
`config.toml` interdit d'enregistrer certains répertoires, avec la même
syntaxe de motifs que `furet remove` :

```toml
exclude_dirs = ['node_modules', 'C:\Windows\*', '*\target\*']
```

Un motif sans `\`, `/` ou `:` porte sur n'importe quel segment du chemin
(`node_modules`, `*appdata*`) ; un motif de chemin doit être absolu
(`C:\Windows\*`) ou commencer par `*` (`*\target\*`). Préférez les chaînes
littérales TOML `'C:\...'` (ou écrivez les séparateurs avec `/`).
`furet add`, le fallback disque de `furet query` et `furet import zoxide`
ignorent alors ces répertoires ; un répertoire déjà connu reste en
revanche dans la base jusqu'à un `furet remove <pattern>`.

## Choisir interactivement avec `fi`

`fi` affiche les candidats classés et vous laisse choisir (menu `fzf` si
installé, sinon un menu numéroté dans la console) :

```powershell
PS C:\dev\furet> fi banque
Choose a directory:
  1) C:\dev\formation_dotnet\BanqueDLL
  2) C:\dev\formation_dotnet\BanqueDllTests
Enter to confirm, Esc to cancel
```

Appelé sans argument, `fi` propose tous les répertoires connus, du plus
pertinent au moins pertinent :

```powershell
PS C:\dev\furet> fi
```

Avec `fzf` installé, un panneau d'aperçu s'ouvre à droite de la liste
(`furet preview {}`, moitié droite de l'écran) : il affiche le contenu du
répertoire surligné — d'abord les sous-dossiers (terminés par `\`), puis
les fichiers, tri sans tenir compte de la casse, au plus 50 lignes puis
`… +<K> more`. Un chemin absent ou un fichier affiche `(not a directory)`.
Ce panneau n'existe pas dans le menu numéroté de la console.

### La mémoire des requêtes

Quand une requête vous a déjà mené quelque part, la même requête y
retourne en priorité, même si un autre répertoire correspond mieux.
`c:\om` correspond mieux à `om` que `c:\dev\ombi` ; mais une fois
`ombi` choisi pour `om` :

```powershell
PS C:\dev\furet> f om
PS C:\om>
PS C:\om> fi om                # choix de C:\dev\ombi dans fzf ou le menu
PS C:\dev\ombi> cd \
PS C:\> f om
PS C:\dev\ombi>
```

La casse, les accents et les espaces en trop ne comptent pas (`f OM`,
`f " om "` retournent aussi dans `ombi`), l'ordre des fragments si
(`f io tok` n'est pas `f tok io`). C'est le seul critère placé avant le
score, et il ne s'applique que si le répertoire correspond encore à la
requête et reste candidat (ni le répertoire courant, ni disparu, ni hors
du projet avec `-l`) ; le repli disque n'est jamais concerné. Un saut suivi
en moins de 10 secondes d'un `f -` ou d'un départ ailleurs (un échec
probable) annule la mémoire. La complétion Tab et `fi` proposent le
répertoire mémorisé en premier, et `f om --explain` affiche une ligne
`memory:` :

```text
memory: C:\dev\ombi (chosen 2026-09-27T10:12:40)
```

Pour la désactiver, dans `config.toml` :

```toml
query_memory = false
```

## Les alias

Un alias est un nom court pointant vers un répertoire précis ; il court-
circuite le classement (pas de score, pas de repli disque). Ils se gèrent
avec `furet alias` :

```powershell
PS C:\dev\furet> furet alias add ombi C:\apps\ombi
alias ombi -> C:\apps\ombi

PS C:\apps\ombi> furet alias add outils
alias outils -> C:\apps\ombi\outils

PS C:\dev\furet> furet alias list
ombi	C:\apps\ombi	2026-10-02T09:14:31

PS C:\dev\furet> furet alias remove ombi
removed alias ombi
```

Sans chemin, `alias add` prend le répertoire courant. Un nom qui existe
déjà est refusé (`alias 'ombi' already exists (C:\apps\ombi); use --force
to replace it`) ; `--force` remplace nom et chemin. Les noms acceptés sont
les lettres, chiffres, `_` et `-`. `alias list` écrit sur `stdout` (une
exception documentée), `nom<TAB>chemin<TAB>date de création` par ligne.

Le saut se fait avec le préfixe `!` :

```powershell
PS C:\dev\furet> f !ombi
PS C:\apps\ombi>

PS C:\dev\furet> f !OMBI
PS C:\apps\ombi>
```

La recherche est exacte, sans tenir compte de la casse — pas de flou. Un
saut par alias n'écrit **aucune** ligne de mémoire des requêtes (D1 reste
intact) ; le hook enregistre en revanche la visite `jump` comme pour tout
saut. Les erreurs sont explicites :

```text
furet: unknown alias 'omb'; did you mean 'ombi'?
furet: alias 'ombi' points to a missing directory: C:\apps\ombi
furet: an alias takes no other token
furet: --local cannot be combined with an alias
furet: --home cannot be combined with an alias
```

Le `did you mean` ne suggère un nom qu'à une édition près (une
transposition compte pour une). Un répertoire littéralement nommé `!ombi`
dans le répertoire courant gagne sur l'alias (`f !ombi` y va). Le préfixe
se configure avec la clé `alias_prefix` : `"!"` (défaut) ou `"="`. Le `@`
est exclu : il demande AltGr sur un clavier AZERTY et, surtout, c'est
l'opérateur de *splatting* PowerShell — `f @ombi` sans variable `$ombi`
n'enverrait aucun argument à `f`, qui sauterait silencieusement à la
maison.

La complétion Tab propose les alias par préfixe strict : `f !<Tab>` liste
tous les alias, `f !om<Tab>` ceux commençant par `om`. La liste affiche le
chemin visé, mais la touche Entrée n'insère que le nom (`!ombi`) :

```powershell
PS C:\dev\furet> f !om<Tab>   # propose !ombi  C:\apps\ombi, insère !ombi
```

Après `-l` ou `-h`, un mot d'alias ne propose rien.

## Les marques

Une marque est un raccourci numéroté (`1` à `9`) posé à la volée, dans
l'esprit de Vim ; côté pwsh, tout passe par la fonction `fm` :

```powershell
PS C:\dev\furet> fm 1
mark 1 -> C:\dev\furet

PS C:\dev\ombi> fm 3
mark 3 -> C:\dev\ombi

PS C:\dev\furet> fm
1	C:\dev\furet
3	C:\dev\ombi
```

`fm 1` marque le répertoire courant et le remplace **silencieusement**
s'il existait déjà (le `m1` de Vim) ; `fm` sans argument liste les
marques (`:marks`). Le saut se fait avec le même préfixe `!` que les
alias :

```powershell
PS C:\dev\ombi> f !1
PS C:\dev\furet>

PS C:\dev\furet> f !3
furet: mark 3 not set
```

La suppression suit `:delm` : `fm -d 2` retire la marque 2, `fm -d 2-4`
une plage, `fm -d!` toutes les marques (`1`-`9` seulement, jamais un
alias nommé), sans confirmation :

```powershell
PS C:\dev\furet> fm -d 2-4
removed mark 3

PS C:\dev\furet> fm -d!
removed mark 1
```

Pour enchaîner les marques sans les nommer, `fm +` saute à la marque
suivante et `fm -` à la précédente :

```powershell
PS C:\dev\furet> fm +
PS C:\dev\ombi>
```

`fm +` monte dans les chiffres en sautant les trous et reboucle de 9
vers 1 ; `fm -` fait le miroir. Depuis un répertoire sans marque,
`fm +` part de la plus petite et `fm -` de la plus grande ; un
répertoire portant plusieurs marques compte comme la plus petite, et
toute marque pointant sur le répertoire courant est ignorée. Une marque
dont le répertoire a disparu est sautée avec `furet: skipped mark N:
missing directory` sur stderr. Sans aucune marque : `furet: no marks
set` ; quand aucune n'est éligible : `furet: no other mark` — le saut
n'a pas lieu. Contrairement aux commandes `furet mark` directes,
`fm +` et `fm -` enregistrent la visite `jump` comme tout saut.

Les touches **Ctrl+Alt+→** et **Ctrl+Alt+←** font le même saut sans
rien taper : sur une **ligne de commande vide**, elles exécutent
`fm +` / `fm -` (entrée d'historique normale, visite `jump`
enregistrée) ; si la ligne n'est pas vide, elles ne font que sonner,
pour ne jamais perdre le texte saisi. Elles n'existent que quand
PSReadLine est chargé. Tab complète aussi les marques : `f !<Tab>`
liste chaque alias et marque avec son chemin, mais n'insère que le
nom (`!1`).

Divergence assumée avec Vim : ses marques `'0`-`'9` sont remplies par
viminfo/shada avec les dernières positions de sortie ; les marques
`1`-`9` de furet sont posées par l'utilisateur, rien d'autre.

## Commandes `furet` directes (sans le hook pwsh)

Ces sous-commandes sont utiles pour scripter ou déboguer ; elles écrivent
le chemin résultat sur `stdout` uniquement (menus, erreurs et logs vont sur
`stderr`).

### Compléter les sous-commandes de furet

Le script d'installation (`furet init pwsh`) embarque aussi la complétion
Tab des sous-commandes et options de `furet`, sans ligne de plus dans le
`$PROFILE` :

```powershell
PS C:\dev\furet> furet re<Tab>
PS C:\dev\furet> furet remove

PS C:\dev\furet> furet list --<Tab>   # propose --all et --paths
```

### Inspecter le classement sans sauter

```powershell
PS C:\dev\furet> furet query mcp --list
C:\dev\CodeGroups\CodeGroups.Mcp
C:\dev\CodeGroups\CodeGroups.Mcp.Tests
C:\dev\RedditForKarakeep\mcp
```

### Comprendre pourquoi un chemin a gagné

```powershell
PS C:\dev\furet> f mcp --explain
```

Affiche le détail du score (étape 1 vs étape 2, bonus, récence) sur
`stderr`, sans effectuer le saut ni enregistrer de visite. La forme
directe `furet query mcp --explain` fonctionne aussi, sans le hook pwsh.

### Essayer le moteur nucleo

Le moteur de référence reste celui par défaut. Pour comparer avec le
matcher de Helix (`nucleo`) sur une seule requête :

```powershell
PS C:\dev\furet> furet query dev --explain --engine nucleo
PS C:\dev\furet> furet query dev --explain --engine reference
```

Le rapport commence par `engine: nucleo` (ou `engine: reference`). Avec
nucleo, chaque fragment affiche son score (`token 'dev': nucleo 80`), puis
`sum …, floored …` et, s'il est accordé, `folder bonus +2`. Les deux
moteurs peuvent choisir des gagnants différents : pour `dev`, nucleo
préfère `my-dev` (début de mot) alors que le moteur de référence préfère
`d-e-v`.

Pour l'adopter durablement (y compris pour `f` et `fi`, qui ne passent
jamais `--engine`), ajouter dans `config.toml` :

```toml
engine = "nucleo"
```

`--engine` l'emporte toujours sur `config.toml`. Les accents de la requête
sont retirés avant l'appel à nucleo (`réunions` trouve `Reunions`), mais
nucleo a sa propre normalisation des noms : un nom grec ou cyrillique
accentué (`Αθήνα`, `й`) peut lui échapper, alors que `o` trouve `ø`.

### Colorer la liste et ignorer les règles `.gitignore` du fallback disque

```powershell
PS C:\dev\furet> furet query sourcier --list --color
PS C:\dev\furet> furet query mesures --no-ignore
```

`--color` habille chaque chemin affiché avec la couleur "répertoire"
(l'entrée `di=`) lue dans la variable d'environnement `LS_COLORS`. Si
`LS_COLORS` n'est pas définie — ce qui est le cas par défaut dans
PowerShell, contrairement à un shell Unix — `--color` ne fait rien et le
chemin s'affiche normalement. Pour la voir en action, définissez d'abord
la variable :

```powershell
PS C:\dev\furet> $env:LS_COLORS = "di=1;36"
PS C:\dev\furet> furet query sourcier --list --color
```

`--no-ignore` ne s'applique qu'au fallback disque (SPEC §11), utilisé
quand aucune entrée connue ne correspond à la requête.

### Enregistrer une visite manuellement

Normalement fait automatiquement par le hook pwsh à chaque `cd`, mais
peut être fait à la main (utile en script ou pour importer un historique) :

```powershell
PS C:\dev\furet> furet add C:\dev\sourcier-mesures --session $PID
```

### Consulter le journal des requêtes

```powershell
PS C:\dev\furet> furet queries --failures
```

`--failures` liste les sauts qui étaient probablement des erreurs
(SPEC §15) — utile pour repérer un mauvais classement à corriger. Les
choix faits dans un menu (`furet query`) ou via `fi` sont aussi journalisés
(`pick`) et comptés comme des sauts par la calibration.

### Lister les répertoires connus

```powershell
PS C:\dev\furet> furet list
C:\dev\CodeGroups\CodeGroups.Mcp	3	2026-09-20T14:12:05	2026-09-01T09:30:00
```

Une ligne par répertoire connu, colonnes séparées par des tabulations :
`path`, `visits` (nombre de visites enregistrées), `last_visit` et
`first_seen`, en heure locale. Les lignes sont triées par ordre
alphabétique des chemins, sans tenir compte de la casse (et `visits` ne
décide plus de l'ordre, contrairement à `furet query --list` sans
requête). Avec `--all`, les répertoires absents du
disque apparaissent aussi, avec une cinquième colonne `present` ou
`missing` :

```powershell
PS C:\dev\furet> furet list --all
```

Avec `--paths` (raccourci `-p`), chaque ligne ne contient que le chemin —
pratique pour rediriger la sortie vers un autre outil sans découper sur
les tabulations ; cela vaut aussi avec `--all`, qui omet alors la colonne
`present`/`missing` :

```powershell
PS C:\dev\furet> furet list --paths
C:\dev\CodeGroups\CodeGroups.Mcp
C:\dev\RedditForKarakeep\mcp
```

### Voir les statistiques

`furet stats` imprime sur `stdout` un aperçu de la base, une ligne
`clé<TAB>valeur` par statistique, dans un ordre fixe :

```powershell
PS C:\dev\furet> furet stats
known_directories	142
missing_directories	3
visits	1204
visits_last_30_days	376
queries	233
queries_last_30_days	88
jumps	61
probable_failures	2
failure_rate	3.3%
stage_1	180
stage_2	29
stage_fallback	17
stage_menu	7
source_hook	1102
source_jump	71
source_back	12
source_up	4
source_fallback	9
source_import	6
top	96	C:\dev\furet
top	41	C:\dev\CodeGroups\CodeGroups.Mcp
top	12	C:\dev\RedditForKarakeep\mcp
```

`failure_rate` rapporte les sauts probablement erronés (`furet queries
--failures`, SPEC §15) au nombre total de sauts, avec une décimale — un
indicateur de la qualité du classement. Puis viennent au plus `n` lignes
`top<TAB><visites><TAB><chemin>` : les répertoires présents (marqueur
`missing_since` absent), même sans aucune visite, les plus visités
d'abord, à égalité par chemin croissant. `--top <n>` change cette limite
(`10` par défaut ; `furet stats --top 0` n'affiche aucune ligne `top`).
La commande est en lecture seule : rien n'est réconcilié ni écrit, les
marqueurs de disparition stockés sont pris tels quels — comme
`furet list`.

### Oublier un répertoire

`furet remove` efface de la base les répertoires connus qui correspondent
au motif. Sans séparateur (`\`, `/` ou `:`), le motif porte sur le **nom**
du répertoire ; sinon, c'est un **chemin**, relatif au répertoire courant.
Le motif de nom est comparé à **chaque segment** du chemin : `ombi*`
sélectionne `ombi` et ses sous-répertoires connus, `*appdata*` tous les
répertoires connus situés sous un dossier `AppData`. En revanche, un motif
de chemin commençant par `*` (`*\cache\*`) est comparé au chemin complet
et n'est jamais ancré au répertoire courant.
`*` matche toute suite de caractères (séparateurs compris), `?` exactement
un caractère, et la casse est ignorée. Tout match est supprimé d'un coup,
sans confirmation, chaque suppression étant signalée sur `stderr` :

```powershell
PS C:\dev\furet> furet remove ombi*
removed C:\apps\ombi
removed D:\x\Ombi-v4
```

Pour oublier d'un coup tous les répertoires connus situés sous un dossier
`AppData`, où qu'ils soient :

```powershell
PS C:\dev\furet> furet remove *appdata*
removed C:\Users\thouz\AppData\Local\sourcier\projects\Ombi-16e1bbba
```

Avec `--confirm`, une question est posée sur `stderr` pour chaque
répertoire : `y`/`yes` le supprime, `n`/`no`/Entrée le garde, `a`/`all`
supprime aussi tous les suivants sans rien demander, `q`/`quit` ou une
fin d'entrée (Ctrl-Z) garde ce répertoire et tous les suivants ; toute
autre réponse repose la même question. Les suppressions retenues sont
appliquées après toutes les questions :

```powershell
PS C:\dev\furet> furet remove ombi* --confirm
Remove C:\apps\ombi? [y/N/a/q] y
Remove D:\x\Ombi-v4? [y/N/a/q] y
removed C:\apps\ombi
removed D:\x\Ombi-v4
```

Pour voir ce qui serait supprimé sans rien toucher à la base,
`--dry-run` imprime `would remove <chemin>` par correspondance et ne pose
aucune question, même combiné à `--confirm` :

```powershell
PS C:\dev\furet> furet remove ombi* --dry-run
would remove C:\apps\ombi
would remove D:\x\Ombi-v4
```

Un répertoire oublié revient dans la base au prochain `furet add`, comme
avec zoxide.

### Nettoyer les dossiers disparus

`furet remove --missing` réconcilie d’abord toute la base avec le disque,
puis cible les répertoires vraiment disparus : un dossier revenu — clé USB
rebranchée, lecteur réseau de retour — n’est jamais supprimé et son
marqueur périmé est effacé. Comme un disque débranché ressemble lui
aussi à un dossier disparu, la question est posée **par défaut**, avec la
date de disparition :

```powershell
PS C:\dev\furet> furet remove --missing
Remove D:\x\Ombi-v4 (missing since 2026-09-25T18:42:10)? [y/N/a/q] y
Remove E:\backup\old (missing since 2026-09-20T09:03:55)? [y/N/a/q] n
removed D:\x\Ombi-v4
```

`--yes` supprime tout sans aucune question, et `--dry-run` montre ce qui
serait supprimé (`would remove <chemin>`) sans rien écrire en base —
même les marqueurs de disparition restent tels quels :

```powershell
PS C:\dev\furet> furet remove --missing --yes
removed D:\x\Ombi-v4
removed E:\backup\old

PS C:\dev\furet> furet remove --missing --dry-run
would remove D:\x\Ombi-v4
would remove E:\backup\old
```

Un motif peut restreindre la sélection : `furet remove ombi* --missing`
ne vise que les disparus dont le chemin correspond au motif.

### Importer la base zoxide

Pour démarrer avec une base déjà remplie plutôt que vide :

```powershell
PS C:\dev\furet> zoxide query -ls | furet import zoxide
```

Les répertoires déjà connus sont ignorés ; relancer la commande ne fait
donc rien de plus (idempotent).

### Remonter ou revenir en arrière sans le hook

```powershell
PS C:\dev\furet> furet up 2
C:\dev

PS C:\dev\furet> furet back --session $PID
C:\dev\clypher
```

### Gérer les marques sans le hook

Une marque est un raccourci numéroté (`1` à `9`) posé à la volée, dans
l'esprit de Vim ; elle partage la table et l'espace de noms des alias :

```powershell
PS C:\dev\furet> furet mark set 1
mark 1 -> C:\dev\furet

PS C:\dev\furet> furet mark set 2 C:\dev\ombi
mark 2 -> C:\dev\ombi

PS C:\dev\furet> furet mark list
1	C:\dev\furet
2	C:\dev\ombi
```

Sans chemin, `mark set` prend le répertoire courant. Reposer une marque
existante la remplace **silencieusement** (comme le `m1` de Vim), là où
`furet alias add 1` refuserait un nom déjà pris sans `--force`. `mark
list` écrit sur `stdout` comme `alias list` (exception documentée), un
`chiffre<TAB>chemin` par ligne. Le saut se fait avec le même préfixe `!`
que les alias :

```powershell
PS C:\dev\ombi> f !1
PS C:\dev\furet>

PS C:\dev\furet> f !3
furet: mark 3 not set
```

Une marque non posée n'a jamais de suggestion `did you mean`. La
suppression suit `:delm` — un chiffre ou une plage, muette sur les
marques non posées, et `--all` ne touche que `1`-`9`, jamais un alias
nommé :

```powershell
PS C:\dev\furet> furet mark delete 2-4
removed mark 2

PS C:\dev\furet> furet mark delete --all
removed mark 1
```

Pour enchaîner les marques sans les nommer, `furet mark next` et
`furet mark prev` écrivent sur `stdout` le chemin de la marque suivante
ou précédente — un saut comme un autre, donc sans nouvelle exception
`stdout`. Avec les marques 1 = `C:\dev\ombi` et 3 = `C:\dev\furet`
reposées :

```powershell
PS C:\dev\ombi> furet mark next
C:\dev\furet

PS C:\dev\furet> furet mark next
C:\dev\ombi

PS C:\dev\furet> furet mark prev
C:\dev\ombi
```

`next` monte dans les chiffres en sautant les trous et reboucle de 9
vers 1 ; `prev` fait le miroir. Depuis un répertoire sans marque,
`next` part de la plus petite et `prev` de la plus grande. Un
répertoire portant plusieurs marques compte comme la plus petite, et
toute marque pointant sur le répertoire courant est ignorée ; une
marque dont le répertoire a disparu est sautée avec
`furet: skipped mark N: missing directory` sur stderr. Deux erreurs,
exit 1 : `furet: no marks set` quand aucune marque n'existe (un alias
nommé ne compte pas) et `furet: no other mark` quand aucune n'est
éligible. Le binaire n'enregistre ni visite ni requête ; c'est le
`fm +` / `fm -` de pwsh qui enregistrera la visite de saut.

La fonction `fm`, qui encapsule ces commandes côté pwsh, est décrite
plus haut dans [Les marques](#les-marques).
