# Dashboard d'Indicateurs Crypto et Financiers

Application de bureau (Linux Debian / Ubuntu et Windows) permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers.

Écrite en **Rust** avec [Tauri](https://tauri.app) : le cœur (`core/`) récupère les données et calcule les indicateurs, l'interface (`ui/`) les affiche avec Plotly.js dans une fenêtre native. La version 1.x était une application Python/Streamlit ; elle reste disponible dans l'historique git.

## Fonctionnalités

L'application propose une barre de navigation latérale pour choisir parmi les outils suivants :

1.  **Indice Fear & Greed** : Visualisation du sentiment de marché corrélé au prix du Bitcoin.
2.  **Bitcoin Halving** : Analyse des cycles de halving avec identification des sommets et des creux de cycle.
3.  **Indicateurs On-chain** : Moyenne mobile 200 semaines (SMA), Pi Cycle Top, et Prix Réalisé (Realized Price).
4.  **Cycle de 4 ans (Bitcoin)** : Graphique polaire interactif divisé en 4 années (quadrants), permettant de suivre la progression du prix par rapport au dernier halving.
5.  **SOPR (STH & LTH)** : Rentabilité des pièces dépensées par les détenteurs à court (STH) et long terme (LTH), via BGeometrics.
6.  **Simulateur d'Investissement** : Simulation d'une stratégie de levier dynamique (x1 -> x2 lors d'une baisse de x%) avec sortie progressive personnalisable (journalière, hebdomadaire ou mensuelle). Tout ticker Yahoo Finance est accepté (BTC-USD par défaut, ETH-USD, AAPL...) : les textes (titre, légendes, résultats, journal, CSV, PDF) reprennent le nom de la valeur choisie, et les halvings ne sont tracés que pour le BTC.
7.  **BTC Price & Volume** : Graphique en chandeliers japonais (candlestick) avec volume coloré (vert pour les hausses, rouge pour les baisses).
8.  **Volatility Compression Ratio (VCR)** : Mesure de la compression de volatilité (ratio 30j/365j) pour anticiper les mouvements explosifs.
9.  **Bitcoin Cycle Correction Analysis** : Analyse comparative de la sévérité des corrections (>15%) pour chaque cycle de halving depuis 2010. Identifie les sommets (Tops) et les creux (Bottoms) historiques.
10. **BTC ETF Holdings** : Bitcoin détenus par les ETF spot, corrélés au prix, via BGeometrics.
11. **Bear Market Support Band** : Indicateur de Benjamin Cowen combinant la SMA 20 semaines et l'EMA 21 semaines pour identifier les phases de marché.
12. **Long/Short Positions (OKX)** : Sentiment des traders sur les contrats perpétuels OKX (part des comptes long et short, ratio, open interest), avec le prix de l'actif superposé.
13. **Bitcoin Market Cycle ROI** : Comparaison de la performance du Bitcoin (ROI) depuis les différents sommets (tops) de cycle historiques.
14. **Realized Cap HODL Waves** : Répartition du Realized Cap par ancienneté des UTXO (bandes d'âge, de moins d'un jour à plus de 10 ans), via BGeometrics.
15. **Net Realized Profit / Loss** : Profit ou perte nets réalisés chaque semaine sur la blockchain, affichés en bulles sur le prix du Bitcoin (vert = profit net, rouge = perte nette, taille proportionnelle au montant), à la manière du graphique Glassnode « Profit Taking ». Permet de voir si une hausse s'accompagne d'une forte prise de profit ou non. Données : NRPL journalier de BGeometrics, cumulé par semaine.
16. **MSTR mNAV** : Cours de Strategy (MSTR), coloré du vert au rouge selon son mNAV (capitalisation boursière rapportée à la valeur des bitcoins détenus), avec la courbe du mNAV et ses zones en dessous. Montre d'un coup d'œil si l'action est sous-cotée ou sur-cotée par rapport à ses bitcoins (voir [Calcul du MSTR mNAV](#calcul-du-mstr-mnav)).

### Calcul du Bitcoin Cycle Correction Analysis

#### 1. Définition d’un cycle
Un cycle est défini comme la période allant du bottom (point bas majeur) au top (point haut majeur) suivant.
Exemple de cycles utilisés :
*   Cycle 2013-2017 : du 15/01/2015 au 17/12/2017
*   Cycle 2018-2021 : du 15/12/2018 au 10/11/2021
*   Cycle 2022-2025 : du 21/11/2022 au 15/10/2025 (top projeté)

#### 2. Détection des corrections au sein d’un cycle
Pour chaque cycle, on parcourt les données jour par jour et on détecte les corrections significatives selon l’algorithme suivant :
Soit $P_i$ le prix de clôture du jour $i$.
On maintient à chaque instant le pic local (peak) :
$$\text{Peak}_i = \max(P_0, P_1, \dots, P_i)$$
Une correction est détectée lorsque :
$$\frac{P_i}{\text{Peak}_i} - 1 \leq -0.15$$
soit une baisse d'au moins 15 % depuis le pic.
On enregistre alors la valeur de la correction :
$$\text{Drop}_i = \left( \frac{P_i}{\text{Peak}_i} - 1 \right) \times 100$$

### 3. Calcul des métriques par cycle

Pour chaque cycle, on obtient une liste de corrections $\lbrace \text{Drop}_1, \text{Drop}_2, \dots, \text{Drop}_n \rbrace$.

On calcule alors :

**Moyenne des corrections :**

$$
\text{Correction Moyenne} = \frac{1}{n} \sum_{k=1}^{n} |\text{Drop}_k|
$$

**Correction la plus forte :**

$$
\text{Correction Maximale} = \max_k (|\text{Drop}_k|)
$$

#### 4. Interprétation
Plus la moyenne des corrections et la correction la plus forte diminuent d’un cycle à l’autre, plus le marché se normalise (maturité croissante). L’indice est donc décroissant par nature avec le temps.

#### 5. Exemple concret
*   **Cycle 2013-2017** : Moyenne (34.8%), Max (61.2%)
*   **Cycle 2022-2025** (en cours) : Moyenne (19.0%), Max (15.0%)
Cela montre une nette réduction de l’amplitude des corrections au fil des cycles.

### Calcul du MSTR mNAV

Cette section décrit précisément ce que calcule l'indicateur (code : [`core/src/indicators/mstr_mnav.rs`](core/src/indicators/mstr_mnav.rs)), à partir de quelles données, avec quelles hypothèses, et quelles erreurs en résultent.

#### 1. Définition

Strategy Inc. (ex-MicroStrategy, MSTR) détient une grande quantité de bitcoins. Sa valeur d'actif net en bitcoins (*Net Asset Value*, NAV) est la valeur de marché de ces bitcoins. Le **mNAV** (*multiple of NAV*) rapporte la valeur que le marché attribue à l'entreprise à cette NAV.

Pour un jour de cotation $t$ :

$$
\text{mNAV}(t) = \frac{\text{Cap}(t)}{\text{NAV}(t)} = \frac{P_M(t) \times N(t)}{H(t) \times P_B(t)}
$$

| Symbole | Grandeur | Unité | Source |
|---|---|---|---|
| $P_M(t)$ | cours de clôture de MSTR | USD / action | Yahoo Finance (`MSTR`) |
| $N(t)$ | actions ordinaires en circulation (classes A + B) | actions | fichier intégré + SEC + `config.ini` |
| $H(t)$ | bitcoins détenus | BTC | fichier intégré + SEC + `config.ini` |
| $P_B(t)$ | cours de clôture du bitcoin | USD / BTC | Yahoo Finance (`BTC-USD`) |

Le résultat est un **nombre sans dimension** : les dollars s'annulent entre numérateur et dénominateur.

**Lectures équivalentes :**
- **Prime sur la NAV** : $\text{mNAV} - 1$. Un mNAV de 1,44 signifie que le marché paie 44 % de plus que la valeur des bitcoins détenus.
- **Par action** : $\text{mNAV} = P_M / \left(\frac{H}{N} \times P_B\right)$, où $\frac{H}{N}$ est le nombre de BTC par action. On compare alors le cours de l'action à la valeur des bitcoins qu'elle représente.
- $\text{mNAV} = 1$ (parité) : l'action vaut exactement ses bitcoins, sans rien pour le reste de l'entreprise (activité logicielle, dette, capacité à émettre).

#### 2. Données

**Cours (Yahoo Finance, API `v8/finance/chart`, pas journalier).** Le programme retient la série `close`, qui est ajustée des divisions d'actions (*splits*) mais pas des dividendes. Chaque horodatage est converti en date calendaire dans le fuseau de la place de cotation (champ `gmtoffset`) :
- pour MSTR, heure de New York ;
- pour BTC-USD, UTC.

**Holdings (fichier [`core/data/mstr_holdings.json`](core/data/mstr_holdings.json)).** Aucune API gratuite et sans clé ne fournit l'historique de $H$ et de $N$. Le fichier a donc été établi à la main à partir des publications de Strategy à la SEC et de ses annonces d'achat :
- 8-K, souvent hebdomadaires, qui annoncent les achats de BTC ;
- 10-Q et 10-K, dont la page de couverture donne le nombre d'actions de classe A et B à une date précise.

Chaque point est un triplet $(t_k, H_k, N_k)$. $H_k$ ou $N_k$ peut être absent (`null`) quand la publication ne le donne pas. Chaque point indique sa source.

**Normalisation du split.** Strategy a divisé son action par 10 le 7 août 2024. Les cours Yahoo ajustés expriment tout l'historique en actions d'après la division. Pour rester cohérent, tous les $N_k$ antérieurs au split ont été multipliés par 10. Le mNAV est **invariant par division d'actions** : un split de facteur $s$ transforme $P_M \to P_M/s$ et $N \to s\,N$, donc $P_M \times N$ ne change pas. Il suffit que les deux grandeurs soient exprimées dans la même base, ce que garantit cette normalisation.

**Mise à jour automatique depuis la SEC (EDGAR).** À chaque affichage de l'indicateur, l'application complète le fichier avec les publications postérieures, sans clé et sans IA. Le code est dans [`core/src/sec.rs`](core/src/sec.rs).
- **BTC détenus :** l'application lit la liste des dépôts de Strategy (`data.sec.gov/submissions`), puis le texte de chaque 8-K déposé depuis la dernière date connue (15 au plus). Le nombre est extrait par motifs fixes : la phrase « held an aggregate of approximately N bitcoins », ou, à défaut, la colonne « Aggregate BTC Holdings » du tableau des achats (plus grand nombre de la ligne de données qui ne soit pas un montant en dollars). Les 8-K qui ne parlent pas de bitcoins sont ignorés. Le point prend la date du 8-K.
- **Actions en circulation :** l'application lit le fait XBRL `dei:EntityCommonStockSharesOutstanding` (page de couverture des 10-Q et 10-K) via l'API `companyconcept`. Les valeurs d'un même dépôt et d'une même date (une par classe) sont additionnées, et celles d'avant le 7 août 2024 multipliées par 10.
- **Contrôle de cohérence :** aux dates présentes à la fois dans le fichier et à la SEC, les nombres d'actions sont comparés. Si l'écart dépasse 2 % (classe d'actions manquante, par exemple), les valeurs SEC sont ignorées.
- **Priorité :** à date égale, le fichier intégré l'emporte sur la SEC, et `config.ini` sur les deux. Seules les dates postérieures au fichier apportent donc du nouveau.
- **Diagnostic :** deux messages s'affichent au-dessus du graphique, un pour les BTC, un pour les actions. En **vert**, la récupération a réussi, avec la dernière valeur, sa date et le contrôle de cohérence. En **rouge**, l'erreur exacte (réseau, code HTTP, réponse illisible, incohérence) ; le fichier intégré est alors utilisé seul.
- **Cache :** les réponses sont gardées 12 h en mémoire. La SEC demande d'identifier l'application dans chaque requête : si elle répond HTTP 403, ajoutez un contact dans `config.ini` :

```ini
[SEC]
user_agent = Votre Nom votre@adresse.fr
```

Le nombre d'actions n'est publié qu'environ une fois par trimestre : cette mise à jour n'élimine pas le biais décrit au §6, elle évite seulement de devoir saisir les nouvelles publications à la main.

**Compléments utilisateur.** La section `[MSTR]` de `config.ini` ajoute des points ou en remplace. La fusion se fait **par date et champ par champ** : une valeur de `config.ini` remplace celle du fichier et de la SEC à la même date, et un champ non fourni garde la valeur précédente.

#### 3. Construction des séries continues $H(t)$ et $N(t)$

Les holdings ne sont connus qu'à des dates discrètes $t_1 < t_2 < \dots < t_K$. Pour les autres jours, deux étapes s'appliquent.

**a) Report champ par champ (*forward-fill*).** Pour chaque date $t_k$ :

$$
\tilde H_k = H_j \text{ avec } j = \max \lbrace i \le k : H_i \text{ publié} \rbrace, \qquad \tilde N_k = N_j \text{ avec } j = \max \lbrace i \le k : N_i \text{ publié} \rbrace
$$

Un point n'est retenu que lorsque les deux champs sont connus. Les points antérieurs à la première publication de $N$ sont écartés.

**b) Fonction en escalier (sans interpolation).** Pour un jour quelconque $t \ge t_1$, avec $k(t) = \max \lbrace k : t_k \le t \rbrace$ :

$$
H(t) = \tilde H_{k(t)}, \qquad N(t) = \tilde N_{k(t)}
$$

$H$ et $N$ sont donc constantes par morceaux, continues à droite, et ne changent qu'aux dates de publication. Avant $t_1$, le mNAV n'est pas défini et le graphique commence au premier point complet (11/08/2020).

Le programme n'interpole pas, pour deux raisons :
- **$H$ varie par sauts** (achats ponctuels, pratiquement jamais de ventes) : une interpolation linéaire attribuerait à l'entreprise des bitcoins qu'elle n'a pas encore achetés.
- **$N$ varie à la fois par sauts** (conversions d'obligations, émissions) **et continûment** (programmes d'émission « at-the-market », ATM). Une interpolation utiliserait une publication future et introduirait un **biais d'anticipation** (*look-ahead bias*) : le mNAV d'une date passée dépendrait d'une information qui n'était pas publique ce jour-là. Le report de la dernière valeur connue ne donne que ce qu'un observateur pouvait savoir à la date $t$ (à la date d'effet près, voir §6).

#### 4. Alignement temporel des cours

MSTR ne cote que les jours ouvrés du NYSE/Nasdaq, alors que le bitcoin cote en continu. Le calcul est fait **sur l'ensemble $\mathcal T$ des jours de cotation de MSTR**. Pour chaque $t \in \mathcal T$ :

$$
P_B(t) = \text{clôture BTC-USD du jour } t, \text{ ou à défaut du dernier jour } d \le t \text{ disponible}
$$

Les week-ends et jours fériés n'ont pas de point : le cours de l'action n'y est pas défini, et un mNAV fondé sur une clôture de vendredi et un BTC de dimanche mélangerait deux instants différents. La série BTC est chargée à partir de 7 jours avant $t_1$, pour qu'un $P_B$ existe toujours au premier jour.

**Décalage intra-journalier.** La clôture de MSTR a lieu à 16 h, heure de New York, soit 20 h ou 21 h UTC selon l'heure d'été. La « clôture » journalière BTC-USD de Yahoo correspond à la fin du jour UTC (minuit). Les deux prix sont donc séparés d'environ 3 à 4 heures. Le bitcoin a une volatilité annualisée d'environ 50 %. Sur $\Delta t \approx 4\,\text{h}$, l'écart-type de sa variation relative vaut environ :

$$
\sigma \sqrt{\Delta t} \approx 0{,}5 \times \sqrt{4 / (365 \times 24)} \approx 0{,}011
$$

Ce décalage ajoute donc au mNAV journalier un bruit de l'ordre de 1 % (écart-type), non biaisé en moyenne. Il est négligeable devant les écarts entre zones (0,5 de mNAV, soit 30 à 50 %).

#### 5. Algorithme

Pour chaque jour $t \in \mathcal T$ avec $t \ge t_1$ :
1. $(P_M, P_B) \leftarrow$ clôtures alignées (§4) ;
2. $(H, N) \leftarrow$ holdings en vigueur à $t$ (§3, recherche dichotomique du dernier $t_k \le t$) ;
3. $\text{mNAV} \leftarrow (P_M \times N) / (H \times P_B)$ en virgule flottante double précision (erreur d'arrondi relative $\sim 10^{-16}$, négligeable) ;
4. le point est gardé seulement si le résultat est fini (division par zéro ou donnée manquante exclues).

**Exemple numérique (prix illustratifs).** Au 28/09/2026, les holdings en vigueur sont :
- $H = 847\,666$ BTC (annonce du 27/09/2026) ;
- $N = 384\,225\,751$ actions (couverture du 10-Q au 24/07/2026).

Avec $P_M = 350$ USD et $P_B = 110\,000$ USD :

$$
\text{Cap} = 350 \times 384\,225\,751 \approx 134{,}48 \text{ Md USD}, \qquad \text{NAV} = 847\,666 \times 110\,000 \approx 93{,}24 \text{ Md USD}
$$

$$
\text{mNAV} = \frac{134{,}48}{93{,}24} \approx 1{,}44
$$

soit la zone « neutre », avec une prime de 44 %.

Chaque action représente $H/N \approx 0{,}002206$ BTC, soit 242,68 USD de bitcoins pour une action cotée 350 USD : $350 / 242{,}68 \approx 1{,}44$.

#### 6. Sensibilité et erreurs

En différentiant le logarithme de la formule :

$$
\frac{\delta\,\text{mNAV}}{\text{mNAV}} \approx \frac{\delta P_M}{P_M} + \frac{\delta N}{N} - \frac{\delta H}{H} - \frac{\delta P_B}{P_B}
$$

Une erreur relative de $x$ % sur l'une des quatre grandeurs produit donc une erreur relative d'environ $x$ % sur le mNAV. Les prix de marché sont exacts à la clôture. L'erreur vient essentiellement des holdings :

- **Retard de publication de $N$ (biais à la baisse).** Strategy émet des actions en continu (programmes ATM), mais $N$ n'est publié qu'environ une fois par trimestre. Entre deux publications, le programme utilise $N_{\text{publié}} < N_{\text{réel}}$, donc le mNAV est **sous-estimé**. Si le nombre réel d'actions a augmenté de 10 % depuis la dernière publication, le mNAV affiché vaut $1/1{,}10 \approx 0{,}91$ fois le vrai, soit 9 % de moins. En particulier, aucun nombre d'actions n'a été trouvé entre le 25/07/2024 et le 31/12/2024. Les émissions massives de fin 2024 (environ +26 % d'actions) n'apparaissent donc qu'au 31/12/2024, et le mNAV de cette période est sous-estimé d'autant.
- **Retard de publication de $H$ (biais à la hausse, plus faible).** Les achats de BTC financés par ces émissions sont annoncés à peu près chaque semaine, donc $H$ est plus à jour que $N$. Le léger retard de $H$ surestime le mNAV. Il compense en partie le biais précédent, sans l'annuler, puisque $N$ est bien plus en retard.
- **Date d'effet.** Un point est daté du jour de l'arrêté (fin de trimestre, date de couverture, date de l'annonce), pas du jour où l'information est devenue publique. Il peut donc être appliqué quelques jours avant sa publication réelle : c'est un léger biais d'anticipation, sans effet visible à l'échelle du graphique.
- **Sources secondaires.** Quelques nombres d'actions (fin 2025, T1 et T2 2026) viennent de sources secondaires arrondies (GuruFocus, AlphaQuery). Les deux points de 2026 tirés du 10-K 2025 et du 10-Q T2 2026 supposent la classe B inchangée. L'arrondi (au plus 0,1 M d'actions sur plus de 300 M) donne une erreur inférieure à 0,05 %. La colonne `source` du fichier le précise.

Un avertissement s'affiche lorsque la dernière donnée de holdings précède de plus de **45 jours** le dernier cours : au-delà, le biais sur $N$ peut devenir significatif. Il suffit alors d'ajouter les dernières valeurs dans `config.ini`.

#### 7. Différence avec le mNAV publié par Strategy

Le programme calcule un mNAV « **capitalisation boursière** » : actions de base en circulation, sans dette.

Strategy publie un mNAV fondé sur la **valeur d'entreprise** (*enterprise value*, EV) :

$$
\text{mNAV}_{\text{Strategy}} = \frac{\text{EV}}{\text{NAV}}, \qquad \text{EV} \approx P_M \times N_{\text{dilué}} + \text{dette} + \text{actions préférentielles} - \text{trésorerie}
$$

- $N_{\text{dilué}}$ inclut notamment les obligations convertibles et les options, donc $N_{\text{dilué}} \ge N$.
- La dette et les actions préférentielles s'ajoutent au numérateur.

$\text{mNAV}_{\text{Strategy}}$ est donc **supérieur** à celui calculé ici. L'écart grandit avec l'endettement.

Les deux mesures répondent à des questions différentes :
- la version « capitalisation » mesure la prime payée par l'actionnaire ordinaire sur les bitcoins ;
- la version « EV » mesure la prime payée par l'ensemble des apporteurs de capitaux.

Les dettes et actions préférentielles ne sont pas disponibles en historique fiable et gratuit : la version « capitalisation » est la seule reproductible à partir de données ouvertes.

#### 8. Zones et couleurs

Chaque jour est classé selon trois seuils croissants $\tau_0 < \tau_1 < \tau_2$ (par défaut $1{,}0$ ; $1{,}5$ ; $2{,}5$, modifiables par `seuils = …` dans `[MSTR]`) :

| mNAV | Zone | Couleur |
|---|---|---|
| $< \tau_0$ (1,0) | sous-coté : l'action vaut moins que ses bitcoins | vert franc |
| $[\tau_0, \tau_1)$ (1,0 – 1,5) | neutre | vert à jaune |
| $[\tau_1, \tau_2)$ (1,5 – 2,5) | sur-coté | orange |
| $\ge \tau_2$ (≥ 2,5) | fortement sur-coté | rouge |

Ces seuils sont **conventionnels**, pas des résultats statistiques. Ils traduisent une lecture courante du marché :
- sous 1, on peut en théorie reproduire l'exposition en achetant l'action plutôt que les bitcoins ;
- au-dessus de 2,5, la prime dépend surtout des anticipations sur la capacité de l'entreprise à émettre de nouvelles actions avec une prime.

La couleur des points du cours suit une **échelle continue** :
- elle va de 0 à $c_{\max} = \tau_2 + (\tau_2 - \tau_1)$, soit 3,5 par défaut ;
- ses paliers tombent exactement sur les seuils ($\tau_i / c_{\max}$ en position relative) et aux milieux de zones ;
- les valeurs supérieures à $c_{\max}$ prennent la couleur maximale.

#### 9. Graphique et configuration

Le graphique du haut affiche le cours de MSTR en échelle logarithmique, chaque jour coloré selon le mNAV, avec une échelle graduée en mNAV. Le prix du BTC peut être ajouté en trait fin sur l'axe de droite.

Le graphique du bas trace le mNAV, avec la parité (1,0) en pointillés et les zones en fond.

Le titre et les métriques indiquent :
- le mNAV actuel ;
- sa zone ;
- les BTC détenus ;
- la date de la dernière donnée de holdings utilisée.

**Ajouter les achats récents** sans recompiler : section `[MSTR]` de `config.ini` (voir [Emplacement des fichiers](#emplacement-des-fichiers)). Une ligne par date, au format `BTC détenus` ou `BTC détenus, actions en circulation`. Les séparateurs de milliers sont acceptés (espace ou `_`). À date égale, ces valeurs remplacent celles du fichier, champ par champ. Les lignes mal formées sont ignorées et signalées dans l'application.

```ini
[MSTR]
2026-10-05 = 850000
2026-10-12 = 851 200, 390_000_000
seuils = 1.0, 1.5, 2.5
```

## Installation

Les fichiers d'installation sont publiés dans les [Releases](https://github.com/neomars/dashboard_crypto/releases) du dépôt :

| Système | Fichier |
|---|---|
| Linux Debian / Ubuntu / Mint (64 bits) | `dashboard-crypto_<version>_amd64.deb` |
| Windows 10 / 11 (64 bits), avec installation | `dashboard-crypto_<version>_x64-setup.exe` |
| Windows 10 / 11 (64 bits), sans installation | `dashboard-crypto_<version>_x64-portable.exe` |

Aucune connexion n'est nécessaire à l'installation ; l'application a besoin d'Internet pour récupérer les données de marché.

### Linux (Debian / Ubuntu)

Un paquet `.deb` est publié dans les [Releases](https://github.com/neomars/dashboard_crypto/releases) du dépôt : `dashboard-crypto_<version>_amd64.deb` (Debian 12+, Ubuntu 22.04+, Mint 21+…, 64 bits).

Double-cliquez sur le fichier pour l'ouvrir dans votre gestionnaire de logiciels, ou en ligne de commande :

```bash
sudo apt install ./dashboard-crypto_<version>_amd64.deb
```

- **Dashboard Crypto** apparaît ensuite dans le menu des applications (commande : `dashboard-crypto`).
- Mise à jour : installez le nouveau `.deb` par-dessus l'ancien. Il remplace aussi l'ancienne version Python (1.x).
- Désinstallation : `sudo apt remove dashboard-crypto`.

### Windows

- **Avec installation** : double-cliquez sur `…_x64-setup.exe`. L'installation se fait pour l'utilisateur courant, sans droits administrateur ; **Dashboard Crypto** apparaît ensuite dans le menu Démarrer. Pour mettre à jour, installez la nouvelle version par-dessus ; pour désinstaller : **Paramètres → Applications**.
- **Sans installation** : lancez directement `…_x64-portable.exe`, depuis n'importe quel dossier (une clé USB par exemple).
- Si Windows affiche « Windows a protégé votre ordinateur » (SmartScreen), cliquez sur **Informations complémentaires**, puis **Exécuter quand même** : l'application n'est pas signée numériquement, ce message est normal.
- L'application utilise **WebView2** (Microsoft Edge), déjà présent sur Windows 10 et 11 à jour. S'il manque, l'installateur le télécharge ; la version portable ne peut pas le faire.

### Emplacement des fichiers

| | Linux | Windows |
|---|---|---|
| Configuration (`config.ini`) | `~/.config/dashboard-crypto/` | `%APPDATA%\dashboard-crypto\` |
| Cache BGeometrics | `~/.cache/dashboard-crypto/bgeometrics/` | `%LOCALAPPDATA%\dashboard-crypto\bgeometrics\` |

L'emplacement de `config.ini` peut être imposé avec la variable d'environnement `DASHBOARD_CRYPTO_CONFIG`. Le chemin exact est affiché en bas de la page **Accueil**.

## Sources de données

Toutes les données viennent d'API publiques et gratuites, **sans clé ni compte** :

| Source | Données |
|---|---|
| [Yahoo Finance](https://finance.yahoo.com) | prix, volumes et historiques (BTC et tout ticker du simulateur) |
| [BGeometrics](https://bitcoin-data.com) | métriques on-chain : STH/LTH-SOPR, NRPL, Realized Cap HODL Waves, BTC détenus par les ETF |
| [OKX](https://www.okx.com) | ratio de comptes long/short et open interest des contrats perpétuels |
| [alternative.me](https://alternative.me/crypto/fear-and-greed-index/) | indice Fear & Greed |
| [mempool.space](https://mempool.space) | hauteur de bloc (estimation du prochain halving) |
| [Bitcoin-Dataset](https://github.com/Yrzxiong/Bitcoin-Dataset) (GitHub) | historique du prix BTC 2010-2018 |
| Strategy / [SEC EDGAR](https://www.sec.gov/edgar) | BTC détenus et actions en circulation de Strategy (MSTR) : fichier intégré, complété automatiquement par les 8-K, 10-Q et 10-K récents, et par `config.ini` |

### Limites de BGeometrics

L'offre gratuite de BGeometrics est limitée à **8 requêtes par heure et 15 par jour**, et couvre les **4 dernières années**. Ses métriques étant mises à jour une fois par jour, l'application garde chaque réponse **12 h sur le disque** (dossier de cache, voir [Emplacement des fichiers](#emplacement-des-fichiers)) : afficher les 4 indicateurs concernés coûte au plus 5 requêtes par demi-journée. Si l'API refuse une requête, les dernières données connues sont affichées, avec leur date.

### Noms des endpoints BGeometrics

Pour chaque métrique, l'application essaie une courte liste de noms d'endpoint connus et mémorise celui qui répond. Si un indicateur ne trouve pas ses données, indiquez le nom exact (visible dans la [documentation de l'API](https://bitcoin-data.com/api/redoc.html)) sur la page **Accueil**, section **Endpoints BGeometrics**. Il est enregistré dans `config.ini` (voir [Emplacement des fichiers](#emplacement-des-fichiers)) :

```ini
[BGEOMETRICS]
etf = etf-btc-total
```

Une variable d'environnement `BGEOMETRICS_<CLÉ>` (ex. `BGEOMETRICS_ETF=etf-btc`) est prioritaire. Clés : `sth_sopr`, `lth_sopr`, `etf`, `realized_cap_hodl_waves`, `nrpl`.

## Développement

### Prérequis

- [Rust](https://rustup.rs) (stable) et [Node.js](https://nodejs.org) 20+ (uniquement pour l'outil Tauri et Plotly.js) ;
- sous Windows : les [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (charge de travail « Développement Desktop en C++ ») et WebView2 ;
- sous Linux, les bibliothèques système de Tauri :
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev libsoup-3.0-dev build-essential curl file patchelf
  ```

### Commandes

```bash
npm install          # outil Tauri + Plotly.js
npm run dev          # lance l'application en mode développement
npm run build        # construit le paquet : .deb sous Linux (target/release/bundle/deb/),
                     # installateur .exe sous Windows (target/release/bundle/nsis/)
cargo test --workspace   # tests (après un premier `npm run vendor`)
```

Les tests couvrent les calculs (fenêtres glissantes, corrections, BMSB, VCR, simulateur, profit/perte réalisés…), le décodage des réponses Yahoo Finance / BGeometrics / OKX / Fear & Greed / SEC (liste des dépôts, actions XBRL, BTC détenus dans les 8-K), le cache disque, le rapport PDF, et le rendu de chaque indicateur à partir de données synthétiques (`core/tests/render_all.rs`, sans réseau). Avec `RENDER_SAMPLES_HTML=/tmp/figures.html`, ce test écrit aussi une page affichant toutes les figures, pour un contrôle visuel.

### Publier une version

Pousser un tag `v2.1.0` : le workflow GitHub `Build` fixe la version (`scripts/set-version.mjs`), lance les tests (sous Linux et sous Windows), construit le `.deb` et les deux `.exe`, vérifie que chacun s'installe et démarre, puis les publie dans une Release. Il tourne aussi sur chaque pull request, dont les paquets sont téléchargeables dans les artefacts du workflow, et peut être lancé manuellement depuis l'onglet Actions.

## Structure du projet

- `core/` : bibliothèque Rust sans interface.
  - `data.rs` : accès aux données (Yahoo Finance, historique BTC 2010-2018, Fear & Greed, mempool.space, BGeometrics, OKX) avec cache mémoire, et cache disque pour BGeometrics.
  - `bgeometrics.rs`, `okx.rs` : décodage des réponses de ces deux API.
  - `sec.rs` : SEC EDGAR (liste des dépôts, nombre d'actions XBRL, BTC détenus lus dans les 8-K).
  - `table.rs` : lecture souple des colonnes des données tabulaires.
  - `indicators/` : un module par indicateur, chacun produisant une figure Plotly (JSON).
  - `simulator.rs` : simulateur de levier dynamique (règle « no-loss », liquidation, export CSV).
  - `pdf.rs` : rapport PDF de simulation.
  - `config.rs` : lecture/écriture de `config.ini` (noms d'endpoint BGeometrics imposés, sections `[MSTR]` et `[SEC]`).
  - `indicators.json` : liste des outils affichés dans la barre latérale.
  - `data/mstr_holdings.json` : BTC détenus et actions en circulation de Strategy (MSTR), avec leurs sources.
- `src-tauri/` : application de bureau (commandes appelées par l'interface) et configuration des paquets : `tauri.conf.json` commun, `tauri.linux.conf.json` (`.deb`), `tauri.windows.conf.json` (installateur NSIS).
- `ui/` : interface (HTML/CSS/JavaScript, sans étape de compilation).
- `scripts/` : copie de Plotly.js dans `ui/vendor/`, changement de version.
