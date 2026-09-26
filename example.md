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
absolu, `/` accepté) ; sans elle, `f` sans argument va vers `$HOME`.

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
(SPEC §15) — utile pour repérer un mauvais classement à corriger.

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
removed C:\Users\thouz\AppData\Local\sourcier\projects\Clyd-16e1bbba
```

Avec `--confirm`, les correspondances sont listées et une confirmation est
demandée avant de supprimer (toute autre réponse que `y` ou `yes`, ligne
vide ou Ctrl-D compris, ne supprime rien) :

```powershell
PS C:\dev\furet> furet remove ombi* --confirm
  C:\apps\ombi
  D:\x\Ombi-v4
Remove 2 directories? [y/N] y
removed C:\apps\ombi
removed D:\x\Ombi-v4
```

Un répertoire oublié revient dans la base au prochain `furet add`, comme
avec zoxide.

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
