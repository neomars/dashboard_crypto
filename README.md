# Dashboard d'Indicateurs Crypto & Finance

Ce projet est une application web interactive construite avec Streamlit permettant de visualiser divers indicateurs des marchés financiers et des cryptomonnaies.

## Fonctionnalités

- **Interface d'accueil** : Une page de bienvenue pour naviguer entre les différents outils.
- **BTC Fear & Greed Index** : Visualisation du cours du Bitcoin (BTC) coloré dynamiquement en fonction de l'indice Fear & Greed (Peur et Cupidité).
- **BTC Halving** : Visualisation des halvings passés et estimation du prochain. Inclut l'identification des sommets de cycle (Tops) et des creux (Bottoms), le calcul des jours après halving pour chaque point clé, et un survol dynamique.
- **On-chain Indicators** : Analyse avancée utilisant des moyennes mobiles clés (200-week SMA, Pi Cycle Top, Realized Price) pour identifier les zones de retournement de marché.
- **Bitcoin 4-Year Cycle** : Une "Hodler's Cheat Sheet" sous forme de graphique polaire pour visualiser les phases psychologiques des cycles de 4 ans.
- **Architecture évolutive** : Facilité d'ajout de nouveaux indicateurs.
- **Optimisation** : Utilisation du cache pour accélérer le chargement des données.

## Installation

### Prérequis

Assurez-vous d'avoir Python 3.8+ installé sur votre machine.

### Étapes d'installation

1. **Cloner le dépôt** (ou télécharger les fichiers) :
   ```bash
   git clone <url-du-depot>
   cd dashboard_crypto
   ```

2. **Créer un environnement virtuel** (recommandé) :
   ```bash
   python -m venv venv
   source venv/bin/activate  # Sur Windows: venv\Scripts\activate
   ```

3. **Installer les dépendances** :
   ```bash
   pip install -r requirements.txt
   ```

## Utilisation

Pour lancer l'application Streamlit, exécutez la commande suivante à la racine du projet :

```bash
streamlit run app.py
```

L'application sera accessible dans votre navigateur à l'adresse par défaut : `http://localhost:8501`.

## Structure du projet

- `app.py` : Le point d'entrée principal de l'application Streamlit.
- `btc_fear_greed.py` : Script pour l'indice Fear & Greed.
- `btc_halving.py` : Script pour l'indicateur de Halving.
- `onchain_indicator.py` : Script pour les indicateurs on-chain.
- `cycle_indicator.py` : Script pour le graphique polaire du cycle de 4 ans.
- `requirements.txt` : Liste des bibliothèques Python nécessaires.
- `README.md` : Instructions d'installation et d'utilisation.

## Auteurs

- [Votre Nom/Pseudo]
