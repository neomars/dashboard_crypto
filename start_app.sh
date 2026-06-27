#!/bin/bash

# Script de lancement automatique pour le Dashboard Bitcoin

# 1. Activation de l'environnement virtuel
if [ -d "btc_env" ]; then
    echo "Activation de l'environnement virtuel 'btc_env'..."
    source btc_env/bin/activate
else
    echo "Attention : l'environnement 'btc_env' n'existe pas."
    echo "Assurez-vous de l'avoir créé avec : python -m venv btc_env"
fi

# 2. Lancement de Streamlit
# --server.headless=false force l'ouverture du navigateur (comportement par défaut en local)
echo "Lancement du dashboard..."
streamlit run app.py --server.headless=false
