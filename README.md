# Dashboard d'Indicateurs Crypto et Financiers

Application de bureau (Linux Debian / Ubuntu et Windows) permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers, ainsi que les signaux et le backtest d'un moteur de trading.

Écrite en **Rust** avec [Tauri](https://tauri.app) : le cœur (`core/`) récupère les données et calcule les indicateurs de marché, le moteur de trading (`trading/`) calcule ses signaux et ses backtests, l'interface (`ui/`) affiche les deux avec Plotly.js dans une fenêtre native. La version 1.x était une application Python/Streamlit ; elle reste disponible dans l'historique git.

## Fonctionnalités

La barre de navigation latérale est divisée en trois sections : **Trading**, **Simulation** et **Indicateurs de marché**.

### Trading (moteur séparé)

Le moteur de trading ([`trading/`](trading/)) est séparé des indicateurs de marché : il ne dépend pas de `core/`, ne lit pas leurs données et ne s'en sert pas pour décider. Il a ses propres données (bougies et funding de Poloniex, la plateforme d'exécution prévue), ses propres indicateurs de signal et ses propres règles de risque ; le tableau de bord ne fait qu'afficher ses résultats. Son objectif se mesure en **nombre de BTC**, pas en dollars : la référence est « garder ses BTC » (×1,00). Il ne passe encore aucun ordre réel (voir [Moteur de trading](#moteur-de-trading)).

- **Signaux de tendance** : score de tendance (−1 à +1) et exposition cible que le moteur viserait à la dernière clôture, pour BTC, ETH, SOL et 8 autres cryptos, en spot ou en perpétuel USDT, sur bougies journalières ou de 4 heures.
- **Backtest tendance** : rejoue le module tendance sur l'historique Poloniex, avec exécution à l'ouverture suivante, frais, glissement, funding, stop suiveur et coupe-circuits ; courbe de capital comparée au Buy & Hold, drawdown, exposition, journal des ordres et des événements de risque, export CSV.

### Simulation et indicateurs de marché

Outils d'analyse : ils ne déclenchent aucun ordre et n'alimentent pas le moteur de trading.

1.  **Indice Fear & Greed** : Visualisation du sentiment de marché corrélé au prix du Bitcoin.
2.  **Bitcoin Halving** : Analyse des cycles de halving avec identification des sommets et des creux de cycle.
3.  **Indicateurs On-chain** : Moyenne mobile 200 semaines (SMA), Pi Cycle Top, et Prix Réalisé (Realized Price).
4.  **Cycle de 4 ans (Bitcoin)** : Graphique polaire interactif divisé en 4 années (quadrants), permettant de suivre la progression du prix par rapport au dernier halving.
5.  **SOPR (STH & LTH)** : Rentabilité des pièces dépensées par les détenteurs à court (STH) et long terme (LTH), via BGeometrics.
6.  **Simulateur d'Investissement** : Simulation d'une stratégie de levier dynamique (x1 -> x2 dès que le cours est à x% ou plus sous le plus haut atteint depuis le début de la période choisie, y compris pendant une phase de levier ; le levier se réactive tant que la condition est remplie) avec sortie progressive personnalisable (journalière, hebdomadaire ou mensuelle). Les réglages (dates, ticker, capital, levier, baisse déclencheur, sortie) sont mémorisés automatiquement dans `config.ini` (section `[SIMULATOR]`) dès qu'une simulation aboutit, et rechargés au lancement suivant. Une date de fin égale à la date du jour est enregistrée comme `end = today` et suit donc le calendrier. Tout ticker Yahoo Finance est accepté (BTC-USD par défaut, ETH-USD, AAPL...) : les textes (titre, légendes, résultats, journal, CSV, PDF) reprennent le nom de la valeur choisie, et les halvings ne sont tracés que pour le BTC.
7.  **BTC Price & Volume** : Graphique en chandeliers japonais (candlestick) avec volume coloré (vert pour les hausses, rouge pour les baisses).
8.  **Volatility Compression Ratio (VCR)** : Mesure de la compression de volatilité (ratio 30j/365j) pour anticiper les mouvements explosifs.
9.  **Bitcoin Cycle Correction Analysis** : Analyse comparative de la sévérité des corrections (>15%) pour chaque cycle de halving depuis 2010. Identifie les sommets (Tops) et les creux (Bottoms) historiques.
10. **BTC ETF Holdings** : Bitcoin détenus par les ETF spot, corrélés au prix, via BGeometrics.
11. **Bear Market Support Band** : Indicateur de Benjamin Cowen combinant la SMA 20 semaines et l'EMA 21 semaines pour identifier les phases de marché.
12. **Long/Short Positions (OKX)** : Sentiment des traders sur les contrats perpétuels OKX (part des comptes long et short, ratio, open interest), avec le prix de l'actif superposé.
13. **Top Traders Long/Short (Binance)** : Cours de la monnaie choisie (chandeliers du contrat perpétuel Binance Futures) au-dessus du ratio long/short des meilleurs traders (les 20 % de comptes ayant le plus de marge), à la manière de CoinGlass (panneau « Meilleurs traders longs/courts (Comptes) »). La monnaie (11 proposées, ou « Autre… » pour tout contrat USDT : ARB, 1000PEPE...) et la période (de 5 minutes à 1 jour) se choisissent en haut de la page, ainsi que l'historique (30 jours, 3 mois, 6 mois ou 1 an) ; l'infobulle donne le cours, le ratio et la part des comptes long et short. L'API de Binance ne publie que les 30 derniers jours du ratio : au-delà, il est lu dans l'archive publique de Binance, après vérification de sa concordance avec l'API (voir [Limites de Binance](#limites-de-binance)).
14. **Bitcoin Market Cycle ROI** : Comparaison de la performance du Bitcoin (ROI) depuis les différents sommets (tops) de cycle historiques.
15. **Realized Cap HODL Waves** : Répartition du Realized Cap par ancienneté des UTXO (bandes d'âge, de moins d'un jour à plus de 10 ans), via BGeometrics.
16. **Net Realized Profit / Loss** : Profit ou perte nets réalisés chaque semaine sur la blockchain, affichés en bulles sur le prix du Bitcoin (vert = profit net, rouge = perte nette, taille proportionnelle au montant), à la manière du graphique Glassnode « Profit Taking ». Permet de voir si une hausse s'accompagne d'une forte prise de profit ou non. Données : NRPL journalier de BGeometrics, cumulé par semaine.
17. **MSTR mNAV** : Cours de Strategy (MSTR), coloré du vert au rouge selon son mNAV (capitalisation boursière rapportée à la valeur des bitcoins détenus), avec la courbe du mNAV et ses zones en dessous. Montre d'un coup d'œil si l'action est sous-cotée ou sur-cotée par rapport à ses bitcoins (voir [Calcul du MSTR mNAV](#calcul-du-mstr-mnav)).
18. **Business Cycle vs Bitcoin** : Momentum du Bitcoin (écart à sa moyenne mobile 24 mois) comparé au cycle économique américain (ISM Manufacturing PMI centré sur 50), avec les périodes de divergence en fond. Reprend l'indicateur TradingView « deKoder | Business Cycle vs Bitcoin » (dK=PMIvsBTC) (voir [Calcul du Business Cycle vs Bitcoin](#calcul-du-business-cycle-vs-bitcoin)).

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
- **Actions en circulation :** l'application lit la page de couverture des 10-Q et 10-K déposés depuis la dernière date connue (3 au plus), ainsi que du dernier déjà couvert par le fichier, qui sert de contrôle. Le nombre vient des balises XBRL intégrées `dei:EntityCommonStockSharesOutstanding` (une par classe d'actions, additionnées, datées par leur contexte XBRL), ou à défaut de la phrase « As of …, the registrant had … shares of class A common stock … and … shares of class B common stock ». Les valeurs d'avant le 7 août 2024 sont multipliées par 10. L'API XBRL `companyconcept` de la SEC ne convient pas : elle n'expose pas les valeurs déclarées par classe et répond HTTP 404 pour Strategy.
- **Contrôle de cohérence :** aux dates présentes à la fois dans le fichier et à la SEC, les nombres d'actions sont comparés. Si l'écart dépasse 2 % (classe d'actions manquante, par exemple), les valeurs SEC sont ignorées.
- **Priorité :** à date égale, le fichier intégré l'emporte sur la SEC, et `config.ini` sur les deux. Seules les dates postérieures au fichier apportent donc du nouveau.
- **Diagnostic :** deux messages s'affichent au-dessus du graphique, un pour les BTC, un pour les actions. En **vert**, la récupération a réussi, avec la dernière valeur, sa date et le contrôle de cohérence. En **rouge**, l'erreur exacte (réseau, code HTTP, réponse illisible, incohérence) ; le fichier intégré est alors utilisé seul.
- **Cache :** les réponses sont gardées 12 h en mémoire.
- **Contact SEC :** la SEC exige que chaque requête indique une adresse e-mail de contact ; sans elle, elle répond HTTP 403 (message rouge). Saisissez-la une fois dans le champ **E-mail de contact SEC**, au-dessus du graphique MSTR mNAV (l'indicateur se recharge aussitôt) ou sur la page **Accueil**. Elle n'est envoyée qu'à la SEC, précédée du nom de l'application, et enregistrée dans `config.ini` :

```ini
[SEC]
user_agent = votre@adresse.fr
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

### Calcul du Business Cycle vs Bitcoin

Code : [`core/src/indicators/pmi_btc.rs`](core/src/indicators/pmi_btc.rs). Les calculs reprennent ceux de l'indicateur TradingView open source « deKoder | Business Cycle vs Bitcoin », réglages par défaut (24, 3, 0,15), sur des données **mensuelles** :

$$
\text{Momentum BTC}(m) = \left( \frac{C(m)}{\text{SMA}_{24}(C)(m)} \times 100 - 100 \right) \times 0{,}15
\qquad
\text{Vague PMI}(m) = (\text{PMI}(m) - 50) \times 3
$$

- $C(m)$ est la dernière clôture BTC du mois $m$ (le mois en cours compte avec la dernière clôture connue, comme une bougie mensuelle) et $\text{SMA}_{24}$ sa moyenne sur 24 mois, réglable de 6 à 60 mois dans l'interface.
- Le PMI est centré sur 50, la frontière entre expansion et contraction de l'industrie américaine. Le facteur 3 le met à l'échelle du momentum BTC. Exemple : PMI d'août 2026 = 54,6, soit une vague de $(54{,}6 - 50) \times 3 = 13{,}8$.
- Couleurs : momentum BTC jaune au-dessus de 0, orange en dessous ; PMI vert-bleu au-dessus de 0, rouge en dessous.
- **Fonds de divergence** : vert-bleu quand le momentum BTC est positif alors que le PMI est négatif (le BTC anticipe une reprise) ; rouge sombre quand le momentum BTC est négatif alors que le PMI est positif (le BTC alerte d'un retournement).

**Données du PMI.** L'ISM ne publie pas d'API gratuite, et la série ISM de FRED a été retirée. L'application combine trois sources, la suivante remplaçant la précédente pour un même mois :
1. un fichier intégré, [`core/data/ism_pmi.csv`](core/data/ism_pmi.csv), de janvier 2013 à août 2026 ;
2. un CSV public mis à jour chaque mois ([sofiaverma06/ism-feed](https://github.com/sofiaverma06/ism-feed)), lu à chaque affichage et gardé 12 h en mémoire ;
3. la section `[PMI]` de `config.ini`, pour ajouter ou corriger un mois sans recompiler.

Le CSV en ligne est contrôlé : les valeurs hors de la plage 25-80 sont rejetées, et s'il diffère du fichier intégré de plus de 1,5 point sur un mois commun, il est ignoré. Un message **vert** (dernier PMI, nombre de nouveaux mois, écart maximal constaté) ou **rouge** (erreur exacte, fichier intégré utilisé) s'affiche au-dessus du graphique. Un avertissement s'affiche aussi si le dernier PMI connu a plus d'un mois de retard sur sa date de publication (1er jour ouvré du mois suivant).

```ini
[PMI]
2026-09 = 49.1
```

## Moteur de trading

Code : [`trading/`](trading/) (crate `trading-engine`). Il ne dépend pas de `core/` : indicateurs de marché et moteur de trading ne partagent ni données ni calculs. L'application ([`src-tauri/src/trading.rs`](src-tauri/src/trading.rs)) appelle le moteur et affiche ses résultats dans la section **Trading**.

### Données

API publique de Poloniex, sans clé ni compte ([`trading/src/poloniex.rs`](trading/src/poloniex.rs)) :

| Données | Endpoint | Limites |
|---|---|---|
| Bougies spot | `GET /markets/{symbole}/candles` (ex. `BTC_USDT`) | 500 bougies par requête ; historique BTC vérifié depuis janvier 2017 |
| Bougies du perpétuel | `GET /v3/market/candles` (ex. `BTC_USDT_PERP`) | 500 bougies par requête ; historique vérifié depuis janvier 2023 |
| Bougies 1 minute (perpétuel et comptant) | mêmes endpoints, intervalle `MINUTE_1` (aussi `MINUTE_15`, `HOUR_1`) | 500 bougies par requête, soit environ 1 050 requêtes par an ; **profondeur réelle non documentée**, détectée au téléchargement ; gardées sur disque (voir Fibonacci à levier) |
| Fiche du contrat | `GET /v3/market/allInstruments?symbol=…` : `maxLever`, `ctVal`, `lotSz`, `minSz`, `tSz`, `mM` | lue au plus une fois par jour, gardée sur disque |
| Paliers de marge de maintenance | `GET /v3/market/riskLimit?symbol=…` | format non vérifié : lecture tolérante, repli sur `mM` |
| Prix de marque | `GET /v3/market/markPriceCandlesticks` | facultatif ; format non vérifié |
| Funding du perpétuel | `GET /v3/market/fundingRate/history` | un taux toutes les 8 h, 100 par requête, **180 derniers jours seulement** |

Le module d'accumulation BTC utilise en plus le MVRV du bitcoin, dans deux sources publiques sans clé ([`trading/src/mvrv.rs`](trading/src/mvrv.rs)) :

| Données | Source | Remarques |
|---|---|---|
| MVRV, historique | CSV public Coin Metrics `https://raw.githubusercontent.com/coinmetrics/data/master/csv/btc.csv`, colonnes `time` et `CapMVRVCur` lues par nom d'en-tête | environ 2,5 Mo, un point par jour depuis 2010 ; fichier mis à jour irrégulièrement (il s'arrêtait au 24/05/2026 lors de la vérification) |
| MVRV, données récentes | API BGeometrics `GET https://bitcoin-data.com/v1/mvrv?startday=…&endday=…` | plan gratuit : 8 requêtes par heure, 15 par jour, 4 dernières années ; les 7 derniers jours sont réservés aux abonnés depuis septembre 2026 (changelog BGeometrics v1.7), donc les données gratuites ont environ une semaine de retard |

Les deux séries sont raccordées : sur les dates communes, la valeur de BGeometrics (la plus récente) l'emporte, et l'écart entre les sources est contrôlé (avertissement au-delà de 2 % en moyenne ou 5 % au maximum). Les données sont gardées en mémoire 6 h (15 min si une source manque). Le moteur limite lui-même ses requêtes BGeometrics (4 par heure, 8 par jour) : le tableau de bord consomme le même quota pour ses propres métriques, via un code distinct. Le nom de l'endpoint et le format de sa réponse viennent de la documentation publique de BGeometrics ; ils n'ont pas pu être vérifiés sur le service lui-même pendant le développement, d'où une lecture tolérante de la réponse (tableau ou enveloppe `data`, date `d` ou `unixTs`, valeur `mvrv`).

Les bougies sont chargées par fenêtres de 500, de la plus récente à la plus ancienne, et gardées 15 min en mémoire. La bougie en cours, pas encore close, est écartée : un signal n'est jamais calculé sur un cours provisoire.

### Module tendance

À la clôture de chaque bougie, le moteur calcule un score $S \in [-1, 1]$ à partir de cette clôture et des bougies précédentes uniquement.

**Variante Donchian (par défaut).** Pour chaque période $L \in \lbrace 20, 55, 100, 200 \rbrace$, un signal $s_L$ vaut :
- $+1$ dès que la clôture dépasse le plus haut des $L$ bougies précédentes, et le reste tant que la clôture tient au-dessus du plus bas des $L/2$ bougies précédentes ;
- $-1$ dans le cas symétrique, sur le perpétuel seulement (pas de vente à découvert en spot) ;
- $0$ sinon.

$$
S = \frac{1}{4} \sum_{L} s_L
$$

**Variante EMA.** $S = +1$ si l'EMA 20 dépasse l'EMA 100 et la clôture la SMA 200 ; $S = -1$ dans le cas inverse (perpétuel seulement) ; $S = 0$ sinon. Les EMA sont amorcées par la moyenne simple de leurs $n$ premières valeurs.

**Variante « cassure 55 jours, sortie sous la moyenne 200 jours »** (`breakout`, celle du backtest par défaut). Acheteuse seulement, même sur le perpétuel, avec un score de 1 ou 0 :
- **entrée** : la clôture égale ou dépasse la plus haute **clôture** des 55 bougies précédentes, **et** dépasse la moyenne simple 200 ;
- **sortie** : la clôture passe sous la moyenne simple 200 ;
- préchauffage : 201 bougies.

### Taille de position et risque

Exposition cible, en fraction du capital (négative pour une vente) :

$$
E = \text{clip}\left(S \times \frac{\sigma^{\ast}}{\hat\sigma_{30}},\ -L_{\max},\ L_{\max}\right)
$$

où $\sigma^{\ast}$ est la volatilité visée (25 % par an par défaut), $\hat\sigma_{30}$ la volatilité réalisée des rendements logarithmiques sur 30 jours (écart-type annualisé sur 365 jours), et $L_{\max}$ le levier maximal : 1 en spot, 2 par défaut en perpétuel, 5 au plus.

- **Rééquilibrage par seuil** : la position n'est ajustée que si elle s'ouvre, se ferme, change de sens, dépasse le levier maximal de plus de 10 % (tolérance : à 2x, au-delà de 2,2x), ou si l'exposition varie de plus de 20 % en relatif. Les petits ajustements coûteraient plus en frais qu'ils ne rapportent ; sans la tolérance, une position à 2x revendait dans chaque creux, puisqu'une baisse la fait grossir.
- **Stop suiveur** : à $3 \times \text{ATR}_{20}$ (lissage de Wilder) du meilleur cours atteint depuis l'entrée ; il ne recule jamais. Après un stop, pas de nouvelle entrée dans le même sens avant une clôture au-delà du meilleur cours d'avant le stop (la tendance reprend) ou un retour du score à 0.
- **Coupe-circuits** : perte de 3 % sur une journée UTC, ou baisse de 10 % depuis le plus haut du capital. Celui de −10 % se déclenche une fois par épisode et ne se réarme qu'après un nouveau plus haut du capital (le compteur affiché est un nombre d'épisodes). En backtest, ils sont signalés ; l'option « Arrêter au premier coupe-circuit » solde la position et arrête tout, comme le fera le moteur en réel (reprise manuelle).
- **Liquidation** (perpétuel) : si, au pire cours de la bougie, le capital tombe sous 0,5 % de la valeur de la position, celle-ci est liquidée et le backtest s'arrête.

### Backtest

Chronologie d'une bougie (aucune décision n'utilise une donnée postérieure à la clôture qui la déclenche) :
1. **ouverture** : un stop franchi par l'écart d'ouverture est exécuté au cours d'ouverture, puis l'ordre décidé à la clôture précédente est exécuté au cours d'ouverture ;
2. **pendant la bougie** : funding, stop touché par le plus bas (ou le plus haut pour une vente), contrôle de liquidation ;
3. **clôture** : valorisation, coupe-circuits, mise à jour du stop, signal et ordre pour l'ouverture suivante.

Coûts :
- **frais** : taux taker sur le montant de chaque ordre, 0,075 % par défaut (le taux le plus élevé relevé pour Poloniex) ;
- **glissement** : cours d'exécution dégradé de 0,05 % par défaut ;
- **funding** (perpétuel) : à chaque paiement de 8 h, la position paie $q \times P \times f$, avec $q$ la quantité signée, $P$ le milieu ouverture-clôture et $f$ le taux. Une position acheteuse paie un taux positif, une position vendeuse le reçoit. Le taux réel est utilisé sur les 180 derniers jours, un taux supposé au-delà (0,01 % par 8 h par défaut) ; le nombre de paiements réels et supposés est affiché ;
- **spot** : pas d'emprunt, chaque achat tient dans la trésorerie, frais compris.

Le Buy & Hold de comparaison achète l'actif au premier cours d'ouverture, frais et glissement compris, et le garde.

**Résultats en BTC (affichés en premier).** À chaque bougie, la valeur du portefeuille en BTC vaut le capital en USDT divisé par le cours de clôture de BTC_USDT (pour un autre actif que BTC, les bougies BTC_USDT du même intervalle sont chargées). La référence « garder ses BTC » achète des BTC au premier cours d'ouverture, frais et glissement compris, et les garde : multiplicateur ×1,00. Sont affichés : BTC au départ et à la fin, multiplicateur, BTC gagnés ou perdus, pire baisse du nombre de BTC (du plus haut au plus bas de la série en BTC) et part du temps hors BTC (bougies sans position, donc en USDT). Le graphique ajoute une ligne « Nombre de BTC » avec la référence « garder » en pointillés, et l'export CSV une colonne `Equity_BTC`. Les cartes en USDT viennent ensuite.

**Valeurs par défaut de l'interface** : bougies 1 jour, spot, variante « cassure », levier 1 (y compris en passant au perpétuel), volatilité visée 100 %, stop 0. En bougies 4 h, un avertissement rappelle que les réglages ont été validés en bougies journalières (les moyennes ne couvrent plus que 3 à 33 jours).

**Périodes courtes.** Sous 365 jours ou sous 5 allers-retours, le backtest avertit (« Période trop courte pour juger une stratégie de tendance ») et n'annualise pas : la carte affiche le rendement total, « non annualisé ».

Mesures, sur 365 jours de cotation par an : rendement annualisé composé, volatilité, Sharpe (taux sans risque nul), Sortino, drawdown maximal, Calmar, temps investi, levier moyen, rotation annuelle, nombre d'allers-retours et part gagnante.

### Accumulation BTC

Page « 🪙 Accumulation BTC » ([`trading/src/accumulation.rs`](trading/src/accumulation.rs)). Elle achète du BTC au comptant à intervalle régulier, en plus grande quantité quand il est bon marché, et **ne vend jamais**.

Paramètres : début, fin (incluse), montant de base par achat (USDT), fréquence (hebdomadaire avec choix du jour, mensuelle avec choix du jour du mois ; un mois plus court est acheté son dernier jour), mode de pondération, paliers modifiables, multiplicateur maximal (plafond de tous les poids), frais, glissement, stack de BTC initial (facultatif).

Le montant d'un achat est $A = A_0 \times w$, où $w$ dépend du mode :

| Mode | Poids $w$ |
|---|---|
| Fixe (référence) | 1 |
| MVRV par paliers | selon le MVRV : ×3 sous 1, ×1,5 sous 2, ×1 sous 3, ×0,5 à partir de 3 (bornes et multiplicateurs modifiables) |
| MVRV inverse | $1 / \text{MVRV}$ |
| Prix / moyenne 200 jours | $\text{SMA}_{200} / \text{clôture}$, calculée sur les bougies Poloniex ; sert aussi de secours sans MVRV |

Il n'y a pas de règle « tout ou rien » du type « acheter seulement si MVRV < 1 » : elle n'aurait rien acheté de 2023 à 2026, le MVRV n'étant descendu qu'à 1,15 au creux de 2026, et les sommets du MVRV baissent à chaque cycle (4,7 en 2017, 4,0 en 2021, 2,8 en 2024). Les paliers réduisent ou augmentent l'achat sans jamais l'interdire.

**Chronologie, sans biais d'anticipation.** Pour un achat le jour $D$, le poids n'utilise que la dernière clôture journalière complète (celle de $D-1$) et le MVRV daté de $D-1$ au plus tard ; l'ordre est exécuté à l'ouverture de la bougie journalière du jour $D$, au prix glissé, les frais étant déduits du montant (BTC reçus $= A (1 - \text{frais}) / \text{prix}$). La bougie du jour n'étant pas close, un achat prévu après la dernière bougie complète n'est pas simulé (avertissement).

**Fraîcheur du MVRV.** Si le MVRV le plus récent a plus de 3 jours, un avertissement s'affiche et les achats non couverts (aucun MVRV daté d'au plus 3 jours avant la veille) passent à la pondération prix / moyenne 200 jours. Avec l'offre gratuite de BGeometrics (7 derniers jours réservés aux abonnés), c'est le cas normal pour les derniers achats.

**Résultats** :
- USDT dépensés, BTC accumulés, coût moyen par BTC (frais et glissement compris) ;
- **BTC en plus à budget égal** (chiffre principal) : écart avec un DCA fixe qui dépense exactement le même total, réparti en montants égaux aux mêmes dates, aux mêmes prix d'exécution ;
- comparaison secondaire : DCA fixe au même montant de base, donc à budget total différent (une pondération qui dépense plus accumule plus sans être meilleure : seule la comparaison à budget égal isole l'apport de la pondération) ;
- graphique en trois rangées : prix en échelle logarithmique avec les achats en marqueurs de taille proportionnelle au montant ; BTC accumulés (pondéré, fixe à budget égal, fixe au montant de base) ; poids de chaque achat et MVRV.

### Fibonacci à levier (backtest)

Page « 📐 Fibonacci à levier (backtest) » ([`fibo.rs`](trading/src/fibo.rs), [`leveraged.rs`](trading/src/leveraged.rs), [`validation.rs`](trading/src/validation.rs)). Elle teste l'idée suivante : ouvrir des trades courts à fort levier (x20 à x75), sur de petits montants, quand le prix revient sur un niveau de retracement de Fibonacci. Le budget de trading est séparé du stock de BTC. **Le module peut conclure « avantage non démontré », et c'est son résultat par défaut.** Le levier ne crée pas d'avantage : il fixe la taille de la position et le prix de liquidation. Chaque trade est donc aussi mesuré en multiples du risque (R), indépendamment du levier, et comparé à des niveaux placebo. Aucun ordre réel, aucune clé API.

**Identification des niveaux, sans biais d'anticipation.** Sur l'unité de détection (15 min, 1 h par défaut, ou 4 h), un zigzag en ligne :
- un pivot n'est *confirmé* qu'à la clôture de la bougie où le prix s'est retourné d'au moins $k \times \text{ATR}_{14}$ ($k = 3$ par défaut, ATR connu avant la bougie) ou d'un pourcentage, depuis l'extrême ; l'instant de confirmation est conservé ; une bougie ne confirme jamais l'extrême qu'elle vient de former ;
- le calcul est en ligne (une bougie après l'autre) : les pivots confirmés avant $t$ sont identiques avec les données jusqu'à $t$ ou avec toutes les données (test `pivots_do_not_change_when_more_data_arrives` ; la simulation complète est testée de la même façon) ;
- le mouvement de référence est formé des deux derniers pivots confirmés, A puis B ; les niveaux valent $B - r\,(B - A)$ pour $r \in \lbrace 0{,}382 ;\ 0{,}5 ;\ 0{,}618 ;\ 0{,}786 \rbrace$ (cases à cocher), dans les deux sens ;
- à la confirmation de B, seuls les niveaux encore devant le prix sont posés ; les autres sont ignorés et comptés ;
- sens « continuation » (acheter le repli d'un mouvement haussier, vendre le rebond d'un baissier) ou « contraire » ; en sens contraire l'entrée est un ordre stop (frais taker, glissement) et le stop et l'objectif sont en miroir (stop au niveau précédent ou en B, objectif en A) ;
- la mise en place est annulée par un nouveau pivot confirmé, un prix au-delà de l'origine A, ou après N bougies de détection (48 par défaut).

Le panneau « Points actuels » montre le dernier mouvement confirmé (dates de A, de B et de la confirmation) et l'état de chaque niveau : en attente, déclenché, invalidé.

**Règles de trade** (simulées bougie 1 minute par bougie 1 minute, les signaux de l'unité de détection n'agissant qu'après leur clôture) :
- **entrée** : ordre limite au niveau (frais maker), exécuté si le plus bas (achat) ou le plus haut (vente) d'une bougie 1 min le touche, jamais à un meilleur prix que le sien ; option prudente : le traverser d'un tick (aussi pour l'objectif) ; le premier niveau touché ouvre le trade, un trade par mouvement, une seule position à la fois ;
- **stop** : au-delà du niveau suivant (entrée à 61,8 % : stop à 78,6 %, puis l'origine A), plus une marge en % ; ou au-delà de l'origine A ; ou à distance fixe en % ; sortie au marché (frais taker, glissement) ;
- **objectif** : retour à B, extension 127,2 % ou 161,8 % du mouvement A→B, ou multiple fixe du risque ; sortie limite (frais maker) ;
- **taille** : marge isolée par trade et levier $L$ (1 au maximum lu sur l'API ; au-delà : refusé). Le montant engagé, marge × $L$, est arrondi vers le bas au nombre entier de contrats (`ctVal`, `lotSz`) ; moins d'un contrat : refusé et compté ;
- **liquidation** (marge isolée) : $\text{entrée} \times (1 - 1/L + m)$ pour un achat, $\text{entrée} \times (1 + 1/L - m)$ pour une vente, $m$ étant la marge de maintenance ; toute la marge isolée est perdue. Si elle tombe avant le stop, le trade est refusé et compté (option : la liquidation devient le stop). Sans historique du prix de marque, elle se lit sur le dernier prix avec une marge de sécurité paramétrable (0,1 %) ;
- **funding** toutes les 8 h sur le montant engagé (taux réel sur 180 jours, sinon taux supposé de 0,01 % par 8 h) ;
- **R** est la perte prévue au stop : quantité × distance entrée-stop, hors frais. L'espérance nette en R inclut frais, glissement et funding ;
- **ruine** : on arrête quand la marge n'est plus finançable (budget initial 500 USDT par défaut) ; elle est affichée ;
- **bougie ambiguë** : si le stop (ou la liquidation) et l'objectif sont touchés dans la même bougie 1 min, on compte la sortie défavorable ; l'objectif n'est jamais évalué dans la bougie d'entrée ; le nombre de ces cas est affiché.

**Frais** : défauts maker 0,02 % et taker 0,05 %, modifiables, **à vérifier** : la grille des perpétuels dépend du compte et n'est pas publique par API (des sources publiques donnent selon la date 0,01 %/0,04 % ou 0,01 %/0,075 %).

**Données 1 minute et cache disque.** Plusieurs années de 1 minute, ce sont des millions de bougies : elles sont gardées dans le dossier de données de l'application (`fibo-candles/`, un fichier binaire par mois et par série, 20 octets par bougie, soit environ 40 Mo pour quatre ans), avec la liste des plages déjà interrogées. Le téléchargement est incrémental et reprenable (relancé, il ne demande que ce qui manque), avec progression et annulation dans l'interface ; il ralentit sur une réponse HTTP 429 puis s'arrête en gardant ce qui est reçu. La bougie en cours n'est jamais stockée. La profondeur réelle de l'historique 1 min est détectée par recherche dichotomique ; si le perpétuel ne remonte pas assez loin, les bougies 1 min du comptant `BTC_USDT` servent de trajectoire avant cette date, avec un avertissement (funding et frais restent ceux du perpétuel). Les paramètres du contrat (levier maximal, `ctVal`, marge de maintenance) sont lus sur l'API.

**Protocole de validation** :
1. **Placebo** : le même moteur est relancé avec des niveaux tirés au hasard, uniformes entre 0,30 et 0,80, graine fixe, 200 tirages (réglables). On affiche la distribution de l'espérance nette par trade et le rang de Fibonacci (part des tirages qu'il bat strictement). Avec des niveaux fixes comparables (0,45 ; 0,55 ; 0,70), le détail par niveau est aussi affiché.
2. **Hors échantillon** : réglages sur la période d'étude (01/01/2023 → 31/12/2024), jugement sur la période de test (01/01/2025 → aujourd'hui), affichées côte à côte. La période de test doit suivre celle d'étude. Chaque période est rejouée séparément, avec quelques jours de préchauffage sans trade.
3. **Monte-Carlo** : les trades de la période de test sont rééchantillonnés avec remise (10 000 tirages, résultat en R et en USDT ensemble) : distribution du budget final, probabilité de passer sous 50 % du budget et de le perdre en entier, pire série de pertes.
4. **Verdict** (période de test seulement) : « avantage non démontré » sauf si l'espérance nette par trade est positive, Fibonacci bat au moins 95 % des tirages placebo et il y a au moins 200 trades ; « pas assez de trades » en dessous de 200.
5. **Même jeu à x1** : le moteur en mode « R seulement » (levier 1, quantité fractionnaire, ni liquidation ni arrondi aux contrats) donne la même espérance en R : le levier change la taille et les frais rapportés à la marge, pas l'avantage.

**Repères de contrôle** :
- une marche aléatoire synthétique (graine fixe) doit donner une espérance brute ≈ 0 R, une espérance nette ≈ −coûts, et, avec un stop à 1R et un objectif à 2R, environ s/(s+t) = 1/3 de trades gagnants (test automatique : 33,2 % sur 687 trades) ; sinon, il y aurait un biais d'anticipation ou une erreur d'exécution ;
- le bouton « Contrôler les repères » de la page mesure, sur les bougies journalières BTC_USDT du 01/05 au 12/10/2026, la part des jours où le prix s'éloigne de l'ouverture de 0,5 %, 0,83 %, 1,5 % et 4,5 % (≈ liquidation à x100, x75, x50 et x20), et la compare à vos repères ;
- calcul d'équilibre (test unitaire) : à x100, objectif +1,5 %, liquidation −0,5 % qui coûte toute la marge, 0,05 % de frais par ordre : il faut environ 43 % de trades gagnants, contre 25 % sans avantage ni frais.

**Performance** : en version release, une simulation sur 3 ans de 1 minute (environ 2 millions de bougies) prend environ 25 ms ; l'étude complète (deux périodes, 200 placebo chacune, Monte-Carlo) environ 1 s sur des données synthétiques (`cargo test --release -p trading-engine --test render_trading fibo_timing -- --ignored --nocapture`).

### Limites

- Un backtest favorable n'est pas une preuve. Les paramètres par défaut sont choisis a priori, d'après la littérature sur le suivi de tendance, et non optimisés sur ces données ; validez-les sur une période non utilisée, puis en paper trading.
- L'exécution réelle des ordres n'est pas encore écrite. Elle réutilisera les mêmes signaux et règles de risque, avec une clé API Poloniex limitée au trading (sans droit de retrait). Vérifiez d'abord que Poloniex peut vous servir dans votre pays (règlement européen MiCA).
- Fibonacci à levier : la formule de liquidation de Poloniex, sa grille de frais, le format de `riskLimit` et du prix de marque, la profondeur de l'historique 1 min et les limites exactes de requêtes n'ont pas pu être vérifiés (voir la liste dans la description de la livraison) ; les paramètres du contrat viennent de l'API et le format de `allInstruments` du code de référence `ccxt`. Un seul taux de maintenance est utilisé si les paliers ne sont pas lisibles.
- Le funding de plus de 180 jours est supposé, et le perpétuel n'a pas d'historique vérifié avant 2023.
- Le MVRV de Coin Metrics et celui de BGeometrics peuvent différer légèrement (calcul de la capitalisation réalisée) : l'écart est mesuré et signalé, pas corrigé.
- L'accumulation pondérée suppose que le MVRV reste un bon indicateur de cycle ; ses sommets baissent à chaque cycle, ce qui est une raison de ne pas en faire une règle tout ou rien. Un résultat passé n'est pas une garantie.

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
| [Binance](https://www.binance.com) | cours (bougies) et ratio long/short des meilleurs traders (comptes) des contrats perpétuels USDT ; son API ne publie que les 30 derniers jours du ratio |
| [Binance, archive publique](https://data.binance.vision) | ratio long/short des meilleurs traders au-delà de 30 jours : un fichier zip par jour et par contrat, un relevé toutes les 5 minutes |
| [alternative.me](https://alternative.me/crypto/fear-and-greed-index/) | indice Fear & Greed |
| [mempool.space](https://mempool.space) | hauteur de bloc (estimation du prochain halving) |
| [Bitcoin-Dataset](https://github.com/Yrzxiong/Bitcoin-Dataset) (GitHub) | historique du prix BTC 2010-2018 |
| [ism-feed](https://github.com/sofiaverma06/ism-feed) (GitHub) | ISM Manufacturing PMI mensuel : fichier intégré depuis 2013, complété par ce CSV public et par `config.ini` |
| Strategy / [SEC EDGAR](https://www.sec.gov/edgar) | BTC détenus et actions en circulation de Strategy (MSTR) : fichier intégré, complété automatiquement par les 8-K, 10-Q et 10-K récents, et par `config.ini` |
| [Poloniex](https://api-docs.poloniex.com) | section Trading uniquement : bougies spot et perpétuel, historique du funding (180 derniers jours) |
| [Coin Metrics](https://github.com/coinmetrics/data) (GitHub) et BGeometrics | section Trading uniquement (Accumulation BTC) : MVRV historique (CSV public) et récent (API ; quota gratuit partagé avec le tableau de bord) |

### Limites de Binance

L'API publique de Binance Futures ne conserve que les **30 derniers jours** du ratio long/short des meilleurs traders, quelle que soit la période choisie (au plus 180 relevés en 4 heures, 30 en 1 jour). Les périodes de moins de 2 heures demandent plusieurs requêtes (500 relevés au plus chacune). Les bougies du cours n'ont pas cette limite : l'application en demande autant de jours que l'historique choisi, plus 3 jours de marge pour que le premier relevé ait déjà un cours, par pages lues vers l'avant jusqu'à la bougie en cours. Les réponses sont gardées 5 minutes en mémoire. Si les bougies sont indisponibles ou incomplètes (elles s'arrêteraient avant la bougie en cours, ce qui donnerait un cours périmé), l'indicateur affiche le ratio seul, avec un avertissement. Binance refuse les connexions depuis certaines zones géographiques : l'indicateur affiche alors une erreur « HTTP 451 ».

**Historique de plus de 30 jours.** Binance publie aussi une archive publique, sans clé : [data.binance.vision](https://data.binance.vision), à raison d'un fichier zip par jour et par contrat (`data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-AAAA-MM-JJ.zip`), avec un relevé toutes les 5 minutes. L'application y lit, pour l'historique choisi (3 mois, 6 mois ou 1 an), les jours qui précèdent la fenêtre de l'API, les ramène aux dates de la période choisie (celles de l'API : multiples de la période depuis minuit UTC) et les met bout à bout avec les relevés de l'API. Les fichiers sont téléchargés 8 à la fois (3 essais chacun) et gardés sur le disque (dossier de cache de l'application, `binance-archive/`) : seul le premier affichage d'un historique est long, et le dossier peut être supprimé sans risque. Si Binance ne répond pas à une première série de requêtes, ou au bout de 2 minutes, l'indicateur renonce et affiche les 30 jours de l'API avec un avertissement.

Cette archive est mal documentée (plusieurs demandes d'éclaircissement sont ouvertes dans [binance/binance-public-data](https://github.com/binance/binance-public-data/issues)), donc rien n'y est supposé :

- **Colonnes.** Elles se repèrent par leur nom d'en-tête. Deux colonnes portent le ratio des meilleurs traders : `count_toptrader_long_short_ratio` et `sum_toptrader_long_short_ratio`. Binance ne dit pas laquelle est le ratio de comptes, ni dans quelle unité. L'application compare donc chacune à l'API sur les premiers jours de celle-ci (de quoi réunir une douzaine de relevés communs), garde celle qui concorde, et **refuse l'archive** si l'écart relatif médian dépasse 10 % ou s'il y a moins de 5 relevés communs : elle affiche alors les 30 jours de l'API avec un avertissement, plutôt que de montrer à la suite une autre grandeur. Quand l'archive est retenue, la note de l'indicateur donne l'écart médian constaté.
- **Dates.** Depuis le 25 juin 2026, les fichiers vont de 00:00 à 23:55 au lieu de 00:05 à 00:00 le lendemain, et le relevé d'une même étiquette diffère d'un pas de 5 minutes ; le relevé de minuit figure dans deux fichiers voisins. Chaque date de la période prend le relevé le plus proche à 5 minutes près, ce qui absorbe ces écarts (de l'ordre de 5 minutes, sans effet visible aux périodes proposées). Les premiers fichiers n'avaient qu'une date par jour : ils donnent un point par jour.
- **Trous.** Des jours ont des relevés manquants ou vides (dénominateur nul) : une date sans relevé voisin est sautée, et le tracé relie les points voisins. Un jour sans fichier (HTTP 404) est normal avant la cotation du contrat : l'historique commence alors à sa cotation. Les autres échecs (HTTP 403, 5xx, réseau) sont comptés et signalés ; ils ne sont pas gardés sur le disque, donc réessayés à l'affichage suivant.
- **Profondeur.** L'historique est limité à 6 000 relevés tracés (250 jours en période d'1 heure, 125 en 30 minutes, 30 en 5 minutes) : l'indicateur le signale.

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

Les tests couvrent les calculs (fenêtres glissantes, corrections, BMSB, VCR, simulateur, profit/perte réalisés…), le décodage des réponses Yahoo Finance / BGeometrics / OKX / Binance / Fear & Greed / SEC (liste des dépôts, nombre d'actions en page de couverture, BTC détenus dans les 8-K), le cache disque, le rapport PDF, l'accès à l'API et à l'archive Binance (ratio et bougies : pagination, fenêtre de 30 jours, fichiers zip quotidiens, raccord et contrôle de concordance, jours absents ou en échec, erreurs HTTP, caches) contre un faux serveur local (`core/tests/binance_http.rs`), et le rendu de chaque indicateur à partir de données synthétiques (`core/tests/render_all.rs`, sans réseau). Avec `RENDER_SAMPLES_HTML=/tmp/figures.html`, ce test écrit aussi une page affichant toutes les figures, pour un contrôle visuel.

Ceux du moteur de trading couvrent ses indicateurs (EMA, ATR, canaux, volatilité), les signaux de cassure, les règles de risque, le backtest (exécution à l'ouverture suivante, frais, spot sans emprunt, stop et reprise, funding réel ou supposé, coupe-circuits), les résultats en BTC, la variante cassure, les mesures de performance, le décodage des réponses Poloniex, la lecture et le raccordement du MVRV (Coin Metrics, BGeometrics, limiteur de requêtes) et l'accumulation (calendrier, paliers, chronologie sans biais d'anticipation, comparaisons à budget égal, secours sans MVRV). les paramètres du contrat et les bougies 1 minute (extraits de l'API), le zigzag non rétroactif, les niveaux, la mise en place et son annulation, les ordres limites, la bougie ambiguë, la liquidation, l'arrondi aux contrats, le funding, les frais maker et taker, le placebo déterministe, le Monte-Carlo, le verdict et les repères de contrôle. `trading/tests/render_trading.rs` rejoue les pages Trading sur un marché et un MVRV synthétiques, sans réseau ; avec `TRADING_SAMPLES_HTML=/tmp/trading.html`, il écrit aussi des pages affichant leurs figures (`/tmp/trading.html`, `/tmp/trading-accumulation.html`).

### Publier une version

Pousser un tag `v2.1.0` : le workflow GitHub `Build` fixe la version (`scripts/set-version.mjs`), lance les tests (sous Linux et sous Windows), construit le `.deb` et les deux `.exe`, vérifie que chacun s'installe et démarre, puis les publie dans une Release. Il tourne aussi sur chaque pull request, dont les paquets sont téléchargeables dans les artefacts du workflow, et peut être lancé manuellement depuis l'onglet Actions.

## Structure du projet

- `core/` : bibliothèque Rust sans interface.
  - `data.rs` : accès aux données (Yahoo Finance, historique BTC 2010-2018, Fear & Greed, mempool.space, BGeometrics, OKX, Binance) avec cache mémoire, et cache disque pour BGeometrics et pour l'archive Binance.
  - `bgeometrics.rs`, `okx.rs`, `binance.rs` : décodage des réponses de ces trois API (`binance.rs` : ratio des top traders et bougies, avec les règles de pagination et la fenêtre de 30 jours, et la lecture de l'archive : zip, CSV, rééchantillonnage et raccord avec l'API).
  - `sec.rs` : SEC EDGAR (liste des dépôts, nombre d'actions lu en page de couverture des 10-Q/10-K, BTC détenus lus dans les 8-K).
  - `table.rs` : lecture souple des colonnes des données tabulaires.
  - `indicators/` : un module par indicateur, chacun produisant une figure Plotly (JSON).
  - `simulator.rs` : simulateur de levier dynamique (règle « no-loss », liquidation, export CSV).
  - `pdf.rs` : rapport PDF de simulation.
  - `config.rs` : lecture/écriture de `config.ini` (noms d'endpoint BGeometrics imposés, sections `[MSTR]`, `[SEC]`, `[PMI]` et `[SIMULATOR]`, réglages mémorisés du simulateur).
  - `indicators.json` : liste des outils affichés dans la barre latérale.
  - `data/mstr_holdings.json` : BTC détenus et actions en circulation de Strategy (MSTR), avec leurs sources.
  - `data/ism_pmi.csv` : ISM Manufacturing PMI mensuel depuis janvier 2013.
- `trading/` : moteur de trading, sans dépendance vers `core/`.
  - `poloniex.rs`, `feed.rs` : API publique de Poloniex (bougies, funding) et cache mémoire.
  - `ta.rs` : indicateurs de signal (moyennes mobiles, ATR, plus hauts et plus bas, volatilité réalisée).
  - `strategy.rs` : module tendance (ensemble Donchian, croisement EMA, cassure 55 j / moyenne 200 j).
  - `risk.rs` : cible de volatilité, plafond de levier, rééquilibrage, stop suiveur, coupe-circuits.
  - `backtest.rs`, `metrics.rs` : simulation barre par barre et mesures de performance.
  - `signals.rs` : signal à la dernière clôture.
  - `fibo.rs`, `leveraged.rs`, `validation.rs`, `fibo_report.rs`, `fibo_chart.rs` : Fibonacci à levier (identification des points, simulation 1 min, placebo, Monte-Carlo, verdict, rapport et figures) ; `contract.rs`, `candle_store.rs`, `fibo_data.rs` : paramètres du contrat, cache disque et téléchargement des bougies 1 minute.
  - `accumulation.rs`, `mvrv.rs` : accumulation de BTC (achats pondérés, jamais de vente) et historique du MVRV (Coin Metrics + BGeometrics).
  - `chart.rs`, `tools.json` : figures et pages de la section Trading.
- `src-tauri/` : application de bureau (commandes appelées par l'interface ; celles de la section Trading dans `src/trading.rs`) et configuration des paquets : `tauri.conf.json` commun, `tauri.linux.conf.json` (`.deb`), `tauri.windows.conf.json` (installateur NSIS).
- `ui/` : interface (HTML/CSS/JavaScript, sans étape de compilation).
- `scripts/` : copie de Plotly.js dans `ui/vendor/`, changement de version.
