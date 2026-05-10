# Dashboard d'Indicateurs Crypto et Financiers

Ce projet est une application interactive basée sur **Streamlit** permettant de visualiser divers indicateurs du marché Bitcoin et des marchés financiers.

## Fonctionnalités

L'application propose une interface de navigation latérale pour choisir parmi les indicateurs suivants :

1.  **Indice Fear & Greed** : Visualisation du sentiment de marché corrélé au prix du Bitcoin.
2.  **Bitcoin Halving** : Analyse des cycles de halving avec identification des sommets et des creux de cycle.
3.  **Indicateurs On-chain** : Moyenne mobile 200 semaines (SMA), Pi Cycle Top, et Prix Réalisé (Realized Price).
4.  **Cycle de 4 ans (Bitcoin)** : Graphique polaire interactif divisé en 4 années (quadrants), permettant de suivre la progression du prix par rapport au dernier halving.
5.  **STH-SOPR (Short Term Holder SOPR)** : Analyse de la rentabilité des détenteurs à court terme via l'API Dune Analytics.

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
query_id = 6987189
```
*Note : Le `query_id` peut varier si la requête originale est supprimée ou rendue privée. Vous pouvez trouver d'autres requêtes publiques sur Dune.com (ex: cherchez "Bitcoin STH-SOPR") et copier l'ID présent dans l'URL : `dune.com/queries/<ID>`.*

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
