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

## Installation

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

### 4. Configuration de l'API Dune (pour le SOPR)
L'indicateur STH-SOPR nécessite une clé API Dune Analytics.
1. Créez un fichier nommé `config.ini` à la racine du projet.
2. Ajoutez-y votre clé API comme suit :
```ini
[DUNE]
api_key = VOTRE_CLE_API_ICI
```

*Note : Le fichier `config.ini` est ignoré par git pour protéger votre clé.*

## Utilisation

Pour lancer l'application, exécutez la commande suivante :

```bash
streamlit run app.py
```

L'interface s'ouvrira automatiquement dans votre navigateur par défaut (généralement à l'adresse `http://localhost:8501`).

## Structure du Projet

- `app.py` : Point d'entrée principal de l'application Streamlit.
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
