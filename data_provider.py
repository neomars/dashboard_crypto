"""Fonctions partagées d'accès aux données : Yahoo Finance, Dune Analytics et
l'historique BTC combiné (dataset GitHub 2010-2018 + Yahoo Finance).

Centraliser ces appels ici évite de dupliquer, dans chaque indicateur, la
normalisation des colonnes yfinance, la gestion des erreurs Dune et le calcul
de la date du prochain halving.
"""
from datetime import datetime, timedelta
from io import StringIO

import pandas as pd
import requests
import streamlit as st
import yfinance as yf

from config_manager import get_dune_api_key

REQUEST_TIMEOUT = 15
OLD_BTC_DATASET_URL = "https://raw.githubusercontent.com/Yrzxiong/Bitcoin-Dataset/master/bitcoin_dataset.csv"

KNOWN_HALVINGS = [
    {"date": "2012-11-28", "block": 210000, "reward": "50 → 25"},
    {"date": "2016-07-09", "block": 420000, "reward": "25 → 12.5"},
    {"date": "2020-05-11", "block": 630000, "reward": "12.5 → 6.25"},
    {"date": "2024-04-20", "block": 840000, "reward": "6.25 → 3.125"},
]


@st.cache_data(ttl=3600)
def get_ticker_history(ticker="BTC-USD", start=None, end=None, period=None, interval="1d"):
    """Télécharge un historique de prix via yfinance et aplatit les colonnes.

    Retourne un DataFrame indexé par date avec des colonnes plates
    (Open, High, Low, Close, Volume, ...), ou un DataFrame vide en cas d'échec.
    """
    kwargs = {"interval": interval, "progress": False}
    if period:
        kwargs["period"] = period
    else:
        kwargs["start"] = start or "2010-01-01"
        if end is not None:
            kwargs["end"] = end

    try:
        df = yf.download(ticker, **kwargs)
    except Exception:
        return pd.DataFrame()

    if df.empty:
        return df

    # yfinance renvoie un MultiIndex (champ, ticker) même pour un seul ticker
    if isinstance(df.columns, pd.MultiIndex):
        df.columns = df.columns.get_level_values(0)

    return df


@st.cache_data(ttl=86400)
def get_combined_btc_history():
    """Historique BTC complet : dataset GitHub (2010-2018) fusionné avec Yahoo Finance (2018+).

    Retourne un DataFrame avec les colonnes ['date', 'close'].
    """
    try:
        response = requests.get(OLD_BTC_DATASET_URL, timeout=REQUEST_TIMEOUT)
        response.raise_for_status()
        btc_old = pd.read_csv(StringIO(response.text))
        btc_old["date"] = pd.to_datetime(btc_old["Date"]).dt.tz_localize(None)
        btc_old["close"] = pd.to_numeric(btc_old["btc_market_price"], errors="coerce")
        btc_old = btc_old[["date", "close"]].dropna().sort_values("date").reset_index(drop=True)
    except Exception:
        btc_old = pd.DataFrame(columns=["date", "close"])

    btc_new = get_ticker_history("BTC-USD", start="2018-01-01")
    if not btc_new.empty:
        btc_new = btc_new[["Close"]].reset_index()
        btc_new.columns = ["date", "close"]
        btc_new["date"] = pd.to_datetime(btc_new["date"]).dt.tz_localize(None)
    else:
        btc_new = pd.DataFrame(columns=["date", "close"])

    btc = pd.concat([btc_old, btc_new], ignore_index=True)
    btc = btc.drop_duplicates(subset="date").sort_values("date").reset_index(drop=True)
    # Filtre les prix <= 0 pour éviter les problèmes d'échelle logarithmique
    btc = btc[btc["close"] > 0].reset_index(drop=True)
    return btc


def estimate_next_halving(fallback_block=840000):
    """Estime la date du prochain halving BTC (~10 min/bloc, tous les 210 000 blocs)."""
    try:
        current_block = int(requests.get(
            "https://mempool.space/api/blocks/tip/height", timeout=10
        ).text.strip())
    except Exception:
        current_block = fallback_block

    block_interval = 210000
    next_halving_block = ((current_block // block_interval) + 1) * block_interval
    blocks_to_next = next_halving_block - current_block
    return datetime.now() + timedelta(days=(blocks_to_next * 10) / (60 * 24))


def get_dune_query_results(query_id, api_key=None, timeout=20):
    """Récupère les lignes d'une requête Dune Analytics avec gestion d'erreur unifiée.

    Retourne un tuple (DataFrame, None) en cas de succès, ou (None, message_erreur) sinon.
    """
    if api_key is None:
        api_key = get_dune_api_key()
    if not api_key:
        return None, "Clé API Dune manquante. Configurez-la dans l'onglet Accueil ou dans config.ini."

    url = f"https://api.dune.com/api/v1/query/{query_id}/results"
    headers = {"X-Dune-API-Key": api_key, "Accept-Encoding": "identity"}

    try:
        response = requests.get(url, headers=headers, timeout=timeout)
    except requests.exceptions.RequestException as e:
        return None, f"Erreur de connexion à l'API Dune : {e}"

    if response.status_code == 401:
        return None, "Erreur API Dune : 401 (Non autorisé). Vérifiez votre clé API dans config.ini ou l'onglet Accueil."
    if response.status_code == 404:
        return None, f"Erreur API Dune : 404 (Non trouvé). La requête avec l'ID {query_id} n'existe pas ou est privée."
    if response.status_code == 400:
        try:
            err_msg = response.json().get("error", response.text)
        except Exception:
            err_msg = response.text
        return None, f"Erreur API Dune : 400 (Requête invalide). Détails : {err_msg}"
    if response.status_code != 200:
        return None, f"Erreur API Dune : {response.status_code}"

    try:
        rows = response.json()["result"]["rows"]
    except Exception as e:
        return None, f"Erreur lors du traitement des données Dune : {e}"

    return pd.DataFrame(rows), None
