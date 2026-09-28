# Dashboard d'Indicateurs Crypto et Financiers

Ce projet est une application interactive basée sur **Streamlit** permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers.

## Fonctionnalités

L'application propose une interface de navigation latérale pour choisir parmi les indicateurs suivants :

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

## Installation sur Debian / Ubuntu (application)

Un paquet `.deb` est publié dans les [Releases](https://github.com/neomars/dashboard_crypto/releases) du dépôt : `dashboard-crypto_<version>_all.deb` (Debian 12+, Ubuntu 22.04+, Mint…).

```bash
sudo apt install ./dashboard-crypto_<version>_all.deb
```

- L'installation crée l'environnement Python de l'application dans `/opt/dashboard-crypto/venv` : **une connexion Internet est nécessaire** pendant l'installation (quelques minutes).
- **Dashboard Crypto** apparaît ensuite dans le menu des applications. Elle s'ouvre dans sa propre fenêtre ; fermer la fenêtre arrête l'application.
- En ligne de commande : `dashboard-crypto` (fenêtre native), `dashboard-crypto --browser` (dans le navigateur par défaut).
- La configuration (clé API Dune, IDs de requêtes) est enregistrée dans `~/.config/dashboard-crypto/config.ini`, le journal du serveur dans `~/.cache/dashboard-crypto/streamlit.log`.
- Mise à jour : installez simplement le nouveau `.deb` par-dessus. Désinstallation : `sudo apt remove dashboard-crypto`.

### Construire le paquet soi-même

```bash
packaging/debian/build_deb.sh          # version lue dans le fichier VERSION
packaging/debian/build_deb.sh 1.2.0    # version explicite
```

Le paquet est créé dans `dist/`. Le workflow GitHub `Build` fait la même chose automatiquement : pousser un tag `v1.2.0` construit la version 1.2.0, vérifie que le paquet s'installe et démarre, puis la publie dans une Release. Il peut aussi être lancé manuellement depuis l'onglet Actions.

## Installation depuis les sources

### 1. Prérequis
Assurez-vous d'avoir Python 3.8+ installé.

### 2. Cloner le dépôt
```bash
git clone <votre-repo>
cd <votre-repo>
```

### 3. Installer les dépendances
```bash
pip install -r requirements.txt
```

### 4. Configuration de l'API Dune
Les indicateurs basés sur Dune Analytics (SOPR, Institutional Holding, Long/Short Whale) nécessitent une clé API.

Vous pouvez configurer cette clé de deux manières :
1.  **Via l'interface** : Dans la barre latérale, sous la section **Configuration**, dépliez "API Dune Analytics" pour saisir et sauvegarder votre clé.
2.  **Via le fichier `config.ini`** : Créez un fichier nommé `config.ini` à la racine du projet et ajoutez-y vos informations :
    ```ini
    [DUNE]
    api_key = VOTRE_CLE_API_ICI
    ```

*Note : Vous pouvez obtenir une clé gratuite sur [dune.com](https://dune.com). Le fichier `config.ini` est ignoré par git pour protéger votre clé.*

### 5. Configuration avancée : IDs des requêtes Dune

Les IDs des requêtes Dune (SOPR, Institutional Holding, Long/Short, Realized Cap UTXO, Net Realized Profit/Loss) ont des valeurs par défaut codées en dur (sauf Net Realized Profit/Loss), mais sont surchargeables sans modifier le code, par ordre de priorité :

1. Variable d'environnement `DUNE_QUERY_<NOM>` (ex : `DUNE_QUERY_SOPR=1234567`)
2. Section `[DUNE_QUERIES]` du fichier `config.ini` :
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

## Utilisation

### Lancement automatique (Recommandé)
Un script de lancement est disponible pour activer l'environnement et lancer l'application d'un coup :
```bash
./start_app.sh
```

### Lancement manuel
Si vous préférez lancer l'application manuellement :
```bash
source btc_env/bin/activate
streamlit run app.py
```

L'interface s'ouvrira automatiquement dans votre navigateur par défaut (généralement à l'adresse `http://localhost:8501`).

## Tests

Le projet inclut une suite de tests unitaires (`pytest`) couvrant la logique de calcul pure (simulateur de levier, détection de corrections BMI, régime BMSB, volatilité VCR, gestion d'erreurs Dune).

```bash
pip install -r requirements-dev.txt
pytest
```

## Structure du Projet

- `app.py` : Point d'entrée principal de l'application Streamlit.
- `data_provider.py` : Fonctions partagées d'accès aux données (Yahoo Finance, Dune Analytics, historique BTC combiné, estimation du prochain halving) — centralise la logique commune à la majorité des indicateurs.
- `config_manager.py` : Lecture/écriture de `config.ini` (clé API Dune, IDs de requêtes Dune configurables).
- `btc_fear_greed.py` : Logique de l'indicateur Fear & Greed.
- `btc_halving.py` : Analyse des cycles de halving.
- `onchain_indicator.py` : Métriques On-chain (SMA 200, Pi Cycle, etc.).
- `cycle_indicator.py` : Graphique polaire du cycle de 4 ans (Plotly).
- `sth_sopr.py` : Récupération et visualisation des données SOPR via Dune.
- `investment_simulator.py` : Logique et graphique du simulateur d'investissement. Inclut une règle "no-loss" qui reporte les sorties si le prix est inférieur au prix d'achat.
- `vcr_indicator.py` : Logique de l'indicateur de compression de volatilité.
- `bmi_indicator.py` : Logique de l'indice de maturation du Bitcoin.
- `btc_institutional.py` : Visualisation des holdings institutionnels.
- `btc_volume.py` : Indicateur de prix et volume coloré.
- `bmsb_indicator.py` : Bear Market Support Band (Benjamin Cowen).
- `long_short_whale.py` : Analyse des positions Long/Short sur GMX V2 via Dune Analytics.
- `cycle_roi_indicator.py` : Comparaison des trajectoires de ROI post-top de cycle.
- `realized_cap_utxo.py` : Répartition du Realized Cap par ancienneté des UTXO via Dune Analytics.
- `net_realized_pnl.py` : Profit/perte réalisés par semaine en bulles sur le prix BTC (requête Dune : `dune_queries/net_realized_pnl.sql`).
- `packaging/debian/` : Paquet Debian (script de construction, lanceur en fenêtre native, entrée de menu, icône).
- `tests/` : Suite de tests unitaires `pytest`.
