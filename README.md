# Dashboard d'Indicateurs Crypto et Financiers

Application de bureau (Linux Debian / Ubuntu) permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers.

Écrite en **Rust** avec [Tauri](https://tauri.app) : le cœur (`core/`) récupère les données et calcule les indicateurs, l'interface (`ui/`) les affiche avec Plotly.js dans une fenêtre native. La version 1.x était une application Python/Streamlit ; elle reste disponible dans l'historique git.

## Fonctionnalités

L'application propose une barre de navigation latérale pour choisir parmi les outils suivants :

1.  **Indice Fear & Greed** : Visualisation du sentiment de marché corrélé au prix du Bitcoin.
2.  **Bitcoin Halving** : Analyse des cycles de halving avec identification des sommets et des creux de cycle.
3.  **Indicateurs On-chain** : Moyenne mobile 200 semaines (SMA), Pi Cycle Top, et Prix Réalisé (Realized Price).
4.  **Cycle de 4 ans (Bitcoin)** : Graphique polaire interactif divisé en 4 années (quadrants), permettant de suivre la progression du prix par rapport au dernier halving.
5.  **SOPR (LTH & STH)** : Analyse de la rentabilité des détenteurs à court (STH) et long terme (LTH) via l'API Dune Analytics.
6.  **Simulateur d'Investissement** : Simulation d'une stratégie de levier dynamique (x1 -> x2 lors d'une baisse de x%) avec sortie progressive personnalisable (journalière, hebdomadaire ou mensuelle).
7.  **BTC Price & Volume** : Graphique en chandeliers japonais (candlestick) avec volume coloré (vert pour les hausses, rouge pour les baisses).
8.  **Volatility Compression Ratio (VCR)** : Mesure de la compression de volatilité (ratio 30j/365j) pour anticiper les mouvements explosifs.
9.  **Bitcoin Cycle Correction Analysis** : Analyse comparative de la sévérité des corrections (>15%) pour chaque cycle de halving depuis 2010. Identifie les sommets (Tops) et les creux (Bottoms) historiques.
10. **BTC Institutional Holding** : Visualisation de l'accumulation de Bitcoin par les institutionnels (ETFs Spot) corrélée au prix, via Dune Analytics.
11. **Bear Market Support Band** : Indicateur de Benjamin Cowen combinant la SMA 20 semaines et l'EMA 21 semaines pour identifier les phases de marché.
12. **Long/Short Positions (GMX V2)** : Analyse du sentiment de marché (Open Interest Long vs Short) sur les marchés perpétuels via Dune Analytics.
13. **Bitcoin Market Cycle ROI** : Comparaison de la performance du Bitcoin (ROI) depuis les différents sommets (tops) de cycle historiques.
14. **Realized Cap - UTXO Age Bands** : Répartition du Realized Cap par ancienneté des UTXO (bandes d'âge, de moins d'un jour à plus de 10 ans) via Dune Analytics.
15. **Net Realized Profit / Loss** : Profit et perte réalisés chaque semaine sur la blockchain, affichés en bulles sur le prix du Bitcoin (vert = profit net, rouge = perte nette, taille proportionnelle au montant), à la manière du graphique Glassnode « Profit Taking ». Permet de voir si une hausse s'accompagne d'une forte prise de profit ou non. Nécessite de créer la requête Dune fournie (voir [Configuration avancée](#5-configuration-avancée--ids-des-requêtes-dune)).

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
- Mise à jour : installez le nouveau `.deb` par-dessus l'ancien. Il remplace aussi l'ancienne version Python (1.x), dont la configuration est reprise.
- Désinstallation : `sudo apt remove dashboard-crypto`.

## Configuration

### Clé API Dune

Les indicateurs basés sur Dune Analytics (SOPR, Institutional Holding, Long/Short, Realized Cap UTXO, Net Realized Profit/Loss) nécessitent une clé API gratuite ([dune.com](https://dune.com)). Saisissez-la sur la page **Accueil**, section **Configuration**.

Elle est enregistrée dans `~/.config/dashboard-crypto/config.ini` (emplacement modifiable avec la variable `DASHBOARD_CRYPTO_CONFIG`) :

```ini
[DUNE]
api_key = VOTRE_CLE_API_ICI
```

### IDs des requêtes Dune

Les requêtes Dune utilisées ont des IDs par défaut, modifiables sans toucher au code, par ordre de priorité :

1. Variable d'environnement `DUNE_QUERY_<NOM>` (ex : `DUNE_QUERY_SOPR=1234567`) ;
2. Page **Accueil** → **IDs des requêtes Dune**, ou section `[DUNE_QUERIES]` de `config.ini` :
   ```ini
   [DUNE_QUERIES]
   sopr = 1234567
   institutional = 2345678
   long_short = 3456789
   realized_cap_utxo = 4567890
   net_realized_pnl = 5678901
   ```
3. Valeur par défaut intégrée au code.

**Net Realized Profit / Loss** n'a pas de requête par défaut : créez une nouvelle requête sur [dune.com](https://dune.com) en y collant le SQL de [`dune_queries/net_realized_pnl.sql`](dune_queries/net_realized_pnl.sql), exécutez-la, puis renseignez son ID sous la clé `net_realized_pnl`. La requête doit renvoyer une colonne de date (`week`, `day`, `date`…) et soit deux colonnes profit/perte, soit une colonne nette ; les montants sont regroupés par semaine.

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

Les tests couvrent les calculs (fenêtres glissantes, corrections, BMSB, VCR, simulateur, profit/perte réalisés…), le décodage des réponses Yahoo Finance / Dune / Fear & Greed, le rapport PDF, et le rendu de chaque indicateur à partir de données synthétiques (`core/tests/render_all.rs`, sans réseau). Avec `RENDER_SAMPLES_HTML=/tmp/figures.html`, ce test écrit aussi une page affichant toutes les figures, pour un contrôle visuel.

### Publier une version

Pousser un tag `v2.1.0` : le workflow GitHub `Build` fixe la version (`scripts/set-version.mjs`), lance les tests, construit le `.deb`, vérifie qu'il s'installe et démarre, puis le publie dans une Release. Il peut aussi être lancé manuellement depuis l'onglet Actions.

## Structure du projet

- `core/` : bibliothèque Rust sans interface.
  - `data.rs` : accès aux données (Yahoo Finance, historique BTC 2010-2018, Fear & Greed, mempool.space, Dune Analytics) avec cache mémoire.
  - `dune.rs` : gestion d'erreur et lecture souple des colonnes des réponses Dune.
  - `indicators/` : un module par indicateur, chacun produisant une figure Plotly (JSON).
  - `simulator.rs` : simulateur de levier dynamique (règle « no-loss », liquidation, export CSV).
  - `pdf.rs` : rapport PDF de simulation.
  - `config.rs` : lecture/écriture de `config.ini`.
  - `indicators.json` : liste des outils affichés dans la barre latérale.
- `src-tauri/` : application de bureau (commandes appelées par l'interface, configuration du paquet).
- `ui/` : interface (HTML/CSS/JavaScript, sans étape de compilation).
- `dune_queries/` : SQL de référence des requêtes Dune à créer soi-même.
- `scripts/` : copie de Plotly.js dans `ui/vendor/`, changement de version.
