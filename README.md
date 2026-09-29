# Dashboard d'Indicateurs Crypto et Financiers

Application de bureau (Linux Debian / Ubuntu) permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers.

Écrite en **Rust** avec [Tauri](https://tauri.app) : le cœur (`core/`) récupère les données et calcule les indicateurs, l'interface (`ui/`) les affiche avec Plotly.js dans une fenêtre native. La version 1.x était une application Python/Streamlit ; elle reste disponible dans l'historique git.

## Fonctionnalités

L'application propose une barre de navigation latérale pour choisir parmi les outils suivants :

1.  **Indice Fear & Greed** : Visualisation du sentiment de marché corrélé au prix du Bitcoin.
2.  **Bitcoin Halving** : Analyse des cycles de halving avec identification des sommets et des creux de cycle.
3.  **Indicateurs On-chain** : Moyenne mobile 200 semaines (SMA), Pi Cycle Top, et Prix Réalisé (Realized Price).
4.  **Cycle de 4 ans (Bitcoin)** : Graphique polaire interactif divisé en 4 années (quadrants), permettant de suivre la progression du prix par rapport au dernier halving.
5.  **SOPR (STH & LTH)** : Rentabilité des pièces dépensées par les détenteurs à court (STH) et long terme (LTH), via BGeometrics.
6.  **Simulateur d'Investissement** : Simulation d'une stratégie de levier dynamique (x1 -> x2 lors d'une baisse de x%) avec sortie progressive personnalisable (journalière, hebdomadaire ou mensuelle).
7.  **BTC Price & Volume** : Graphique en chandeliers japonais (candlestick) avec volume coloré (vert pour les hausses, rouge pour les baisses).
8.  **Volatility Compression Ratio (VCR)** : Mesure de la compression de volatilité (ratio 30j/365j) pour anticiper les mouvements explosifs.
9.  **Bitcoin Cycle Correction Analysis** : Analyse comparative de la sévérité des corrections (>15%) pour chaque cycle de halving depuis 2010. Identifie les sommets (Tops) et les creux (Bottoms) historiques.
10. **BTC ETF Holdings** : Bitcoin détenus par les ETF spot, corrélés au prix, via BGeometrics.
11. **Bear Market Support Band** : Indicateur de Benjamin Cowen combinant la SMA 20 semaines et l'EMA 21 semaines pour identifier les phases de marché.
12. **Long/Short Positions (OKX)** : Sentiment des traders sur les contrats perpétuels OKX (part des comptes long et short, ratio, open interest), avec le prix de l'actif superposé.
13. **Bitcoin Market Cycle ROI** : Comparaison de la performance du Bitcoin (ROI) depuis les différents sommets (tops) de cycle historiques.
14. **Realized Cap HODL Waves** : Répartition du Realized Cap par ancienneté des UTXO (bandes d'âge, de moins d'un jour à plus de 10 ans), via BGeometrics.
15. **Net Realized Profit / Loss** : Profit ou perte nets réalisés chaque semaine sur la blockchain, affichés en bulles sur le prix du Bitcoin (vert = profit net, rouge = perte nette, taille proportionnelle au montant), à la manière du graphique Glassnode « Profit Taking ». Permet de voir si une hausse s'accompagne d'une forte prise de profit ou non. Données : NRPL journalier de BGeometrics, cumulé par semaine.

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
$$\frac{P_i}{\text{Peak}_i} - 1 \leq -0.15 \quad \text{(soit une baisse d'au moins 15 \% depuis le pic)}$$
On enregistre alors la valeur de la correction :
$$\text{Drop}_i = \left( \frac{P_i}{\text{Peak}_i} - 1 \right) \times 100$$

### 3. Calcul des métriques par cycle

Pour chaque cycle, on obtient une liste de corrections $\{\text{Drop}_1, \text{Drop}_2, \dots, \text{Drop}_n\}$.

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

## Installation sur Debian / Ubuntu

Un paquet `.deb` est publié dans les [Releases](https://github.com/neomars/dashboard_crypto/releases) du dépôt : `dashboard-crypto_<version>_amd64.deb` (Debian 12+, Ubuntu 22.04+, Mint 21+…, 64 bits).

Double-cliquez sur le fichier pour l'ouvrir dans votre gestionnaire de logiciels, ou en ligne de commande :

```bash
sudo apt install ./dashboard-crypto_<version>_amd64.deb
```

- **Dashboard Crypto** apparaît ensuite dans le menu des applications (commande : `dashboard-crypto`).
- Aucune connexion n'est nécessaire à l'installation ; l'application a besoin d'Internet pour récupérer les données de marché.
- Mise à jour : installez le nouveau `.deb` par-dessus l'ancien. Il remplace aussi l'ancienne version Python (1.x).
- Désinstallation : `sudo apt remove dashboard-crypto`.

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

### Limites de BGeometrics

L'offre gratuite de BGeometrics est limitée à **8 requêtes par heure et 15 par jour**, et couvre les **4 dernières années**. Ses métriques étant mises à jour une fois par jour, l'application garde chaque réponse **12 h sur le disque** (`~/.cache/dashboard-crypto/bgeometrics/`) : afficher les 4 indicateurs concernés coûte au plus 5 requêtes par demi-journée. Si l'API refuse une requête, les dernières données connues sont affichées, avec leur date.

### Noms des endpoints BGeometrics

Pour chaque métrique, l'application essaie une courte liste de noms d'endpoint connus et mémorise celui qui répond. Si un indicateur ne trouve pas ses données, indiquez le nom exact (visible dans la [documentation de l'API](https://bitcoin-data.com/api/redoc.html)) sur la page **Accueil**, section **Endpoints BGeometrics**. Il est enregistré dans `~/.config/dashboard-crypto/config.ini` (emplacement modifiable avec la variable `DASHBOARD_CRYPTO_CONFIG`) :

```ini
[BGEOMETRICS]
etf = etf-btc-total
```

Une variable d'environnement `BGEOMETRICS_<CLÉ>` (ex. `BGEOMETRICS_ETF=etf-btc`) est prioritaire. Clés : `sth_sopr`, `lth_sopr`, `etf`, `realized_cap_hodl_waves`, `nrpl`.

## Développement

### Prérequis

- [Rust](https://rustup.rs) (stable) et [Node.js](https://nodejs.org) 20+ (uniquement pour l'outil Tauri et Plotly.js) ;
- les bibliothèques système de Tauri :
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev libsoup-3.0-dev build-essential curl file patchelf
  ```

### Commandes

```bash
npm install          # outil Tauri + Plotly.js
npm run dev          # lance l'application en mode développement
npm run build        # construit le paquet : target/release/bundle/deb/
cargo test --workspace   # tests (après un premier `npm run vendor`)
```

Les tests couvrent les calculs (fenêtres glissantes, corrections, BMSB, VCR, simulateur, profit/perte réalisés…), le décodage des réponses Yahoo Finance / BGeometrics / OKX / Fear & Greed, le cache disque, le rapport PDF, et le rendu de chaque indicateur à partir de données synthétiques (`core/tests/render_all.rs`, sans réseau). Avec `RENDER_SAMPLES_HTML=/tmp/figures.html`, ce test écrit aussi une page affichant toutes les figures, pour un contrôle visuel.

### Publier une version

Pousser un tag `v2.1.0` : le workflow GitHub `Build` fixe la version (`scripts/set-version.mjs`), lance les tests, construit le `.deb`, vérifie qu'il s'installe et démarre, puis le publie dans une Release. Il peut aussi être lancé manuellement depuis l'onglet Actions.

## Structure du projet

- `core/` : bibliothèque Rust sans interface.
  - `data.rs` : accès aux données (Yahoo Finance, historique BTC 2010-2018, Fear & Greed, mempool.space, BGeometrics, OKX) avec cache mémoire, et cache disque pour BGeometrics.
  - `bgeometrics.rs`, `okx.rs` : décodage des réponses de ces deux API.
  - `table.rs` : lecture souple des colonnes des données tabulaires.
  - `indicators/` : un module par indicateur, chacun produisant une figure Plotly (JSON).
  - `simulator.rs` : simulateur de levier dynamique (règle « no-loss », liquidation, export CSV).
  - `pdf.rs` : rapport PDF de simulation.
  - `config.rs` : lecture/écriture de `config.ini` (noms d'endpoint BGeometrics imposés).
  - `indicators.json` : liste des outils affichés dans la barre latérale.
- `src-tauri/` : application de bureau (commandes appelées par l'interface, configuration du paquet).
- `ui/` : interface (HTML/CSS/JavaScript, sans étape de compilation).
- `scripts/` : copie de Plotly.js dans `ui/vendor/`, changement de version.
