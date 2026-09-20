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

### Remonter ou revenir en arrière sans le hook

```powershell
PS C:\dev\furet> furet up 2
C:\dev

PS C:\dev\furet> furet back --session $PID
C:\dev\clypher
```
